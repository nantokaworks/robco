//! Base-branch protection probing for the auto-merge gate.
//!
//! GitHub exposes branch protection through two independent APIs: the classic
//! `branches/{branch}/protection` endpoint and the rulesets endpoint
//! `rules/branches/{branch}`, which returns the rules already merged across every
//! ruleset that targets the branch. A repository protected only by rulesets answers
//! `404 Branch not protected` on the classic endpoint, so probing one alone reports
//! genuinely protected repositories as unprotected — both are probed and unioned.

use std::{
    collections::HashMap,
    process::Command,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde_json::Value;

use super::COMMAND_TIMEOUT;
use crate::{
    dropr::canonical_repo,
    overseer::{config::ProtectionMode, exec::run_timeout, ledger::LedgerEntry},
    registry::Registry,
};

const PROTECTION_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// TTL for a plan-level refusal — cached far longer than a verified probe. The
/// condition changes only when an operator upgrades the repository's GitHub plan, a
/// deliberate action rather than something that flips within minutes, so retrying
/// every five minutes would still spend two `gh api` calls for no reason.
const PLAN_UNSUPPORTED_CACHE_TTL: Duration = Duration::from_secs(60 * 60);

/// Reason a branch failed the gate, appended to the `unprotected` hold decision so an
/// operator can see which condition is missing without reading source.
pub(super) const NO_PULL_REQUEST_RULE: &str = "no_pull_request_rule";
pub(super) const NO_REQUIRED_STATUS_CHECKS: &str = "no_required_status_checks";
pub(super) const PROBE_UNAVAILABLE: &str = "probe_unavailable";
pub(super) const UNKNOWN_REMOTE: &str = "unknown_remote";
/// No base branch to probe at all — never a guessed `main` (dropr:503).
pub(super) const BASE_BRANCH_UNKNOWN: &str = "base_branch_unknown";
/// The repository's GitHub plan does not expose branch protection at all — every
/// probe answered `403`, not a timeout or a transient failure. Unlike
/// `PROBE_UNAVAILABLE`, this is cached: retrying it every pass would keep spending
/// `gh api` calls to reconfirm a fact that will not change until the plan does. See
/// `crate::overseer::config::OverseerConfig::allow_unverifiable_protection` for the
/// operator's route past it.
pub(super) const PLAN_UNSUPPORTED: &str = "plan_unsupported";

/// What a (repository, branch, mode) triple was last found to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheState {
    /// The facts satisfied the mode.
    Verified,
    /// Every probe answered `403 plan-level`; see `PLAN_UNSUPPORTED`.
    PlanUnsupported,
}

impl CacheState {
    fn ttl(self) -> Duration {
        match self {
            Self::Verified => PROTECTION_CACHE_TTL,
            Self::PlanUnsupported => PLAN_UNSUPPORTED_CACHE_TTL,
        }
    }
}

/// Memoises the last classification of (repository, branch, mode) triples. Loosening
/// the mode or moving to another base branch is a different question, so it re-probes
/// rather than reusing an answer given for a stricter one. Interior-mutable so several
/// repositories' concurrent auto-merge evaluations can share one `&ProtectionCache`;
/// the mutex guards only the `HashMap`, never `api` below.
#[derive(Default)]
pub(super) struct ProtectionCache(Mutex<HashMap<String, (CacheState, Instant)>>);

impl ProtectionCache {
    fn remembered(&self, key: &str, now: Instant) -> Option<CacheState> {
        self.0.lock().unwrap().get(key).and_then(|(state, at)| {
            (now.saturating_duration_since(*at) < state.ttl()).then_some(*state)
        })
    }

    fn remember(&self, key: String, state: CacheState, now: Instant) {
        self.0.lock().unwrap().insert(key, (state, now));
    }
}

fn cache_key(repo: &str, branch: &str, mode: ProtectionMode) -> String {
    format!("{}\u{1f}{repo}\u{1f}{branch}", mode.label())
}

/// The two protection facts the gate cares about, whatever API reported them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ProtectionFacts {
    pub(super) pull_request: bool,
    pub(super) status_checks: bool,
}

impl ProtectionFacts {
    fn union(self, other: Self) -> Self {
        Self {
            pull_request: self.pull_request || other.pull_request,
            status_checks: self.status_checks || other.status_checks,
        }
    }

    /// `None` when the facts satisfy `mode`, otherwise the failing condition.
    pub(super) fn unmet(self, mode: ProtectionMode) -> Option<&'static str> {
        match mode {
            ProtectionMode::Off => None,
            ProtectionMode::Relaxed if self.pull_request => None,
            ProtectionMode::Relaxed => Some(NO_PULL_REQUEST_RULE),
            ProtectionMode::Required if !self.pull_request => Some(NO_PULL_REQUEST_RULE),
            ProtectionMode::Required if !self.status_checks => Some(NO_REQUIRED_STATUS_CHECKS),
            ProtectionMode::Required => None,
        }
    }
}

/// Facts from `GET /repos/{owner}/{repo}/branches/{branch}/protection`.
pub(super) fn classic_facts(value: &Value) -> ProtectionFacts {
    ProtectionFacts {
        pull_request: value
            .get("required_pull_request_reviews")
            .is_some_and(Value::is_object),
        status_checks: value.get("required_status_checks").is_some_and(|checks| {
            checks.is_object()
                && ["contexts", "checks"].iter().any(|field| {
                    checks
                        .get(field)
                        .and_then(Value::as_array)
                        .is_some_and(|items| !items.is_empty())
                })
        }),
    }
}

/// Facts from `GET /repos/{owner}/{repo}/rules/branches/{branch}`, a flat array of
/// `{type, ruleset_id, parameters}` already merged across every ruleset targeting the
/// branch.
pub(super) fn ruleset_facts(value: &Value) -> ProtectionFacts {
    let Some(rules) = value.as_array() else {
        return ProtectionFacts::default();
    };
    ProtectionFacts {
        pull_request: rules
            .iter()
            .any(|rule| rule_type(rule) == Some("pull_request")),
        status_checks: rules.iter().any(|rule| {
            rule_type(rule) == Some("required_status_checks")
                && rule
                    .get("parameters")
                    .and_then(|parameters| parameters.get("required_status_checks"))
                    .and_then(Value::as_array)
                    .is_some_and(|checks| !checks.is_empty())
        }),
    }
}

fn rule_type(rule: &Value) -> Option<&str> {
    rule.get("type").and_then(Value::as_str)
}

/// Probes `branch` in the entry's repository. Returns `None` when the branch satisfies
/// `mode`, otherwise the failing condition.
pub(super) fn unmet_condition(
    entry: &LedgerEntry,
    registry: &Registry,
    cache: &ProtectionCache,
    mode: ProtectionMode,
    branch: &str,
) -> Option<&'static str> {
    if mode == ProtectionMode::Off {
        return None;
    }
    let now = Instant::now();
    let key = cache_key(&entry.repo, branch, mode);
    match cache.remembered(&key, now) {
        Some(CacheState::Verified) => return None,
        Some(CacheState::PlanUnsupported) => return Some(PLAN_UNSUPPORTED),
        None => {}
    }
    let Some(name) = github_slug(entry, registry) else {
        return Some(UNKNOWN_REMOTE);
    };
    let mut facts = ProtectionFacts::default();
    let mut probed = false;
    let mut plan_unsupported = false;
    match api(
        &entry.repo,
        &format!("repos/{name}/rules/branches/{branch}"),
    ) {
        Probe::Answered(value) => {
            probed = true;
            facts = facts.union(ruleset_facts(&value));
        }
        Probe::PlanUnsupported => plan_unsupported = true,
        Probe::Unavailable => {}
    }
    // The rules endpoint never reports classic protection, so a branch the rulesets do
    // not already cover still needs the classic probe before it can be refused.
    if facts.unmet(mode).is_some() {
        match api(
            &entry.repo,
            &format!("repos/{name}/branches/{branch}/protection"),
        ) {
            Probe::Answered(value) => {
                probed = true;
                facts = facts.union(classic_facts(&value));
            }
            Probe::PlanUnsupported => plan_unsupported = true,
            Probe::Unavailable => {}
        }
    }
    match classify(facts, mode, probed, plan_unsupported) {
        None => {
            cache.remember(key, CacheState::Verified, now);
            None
        }
        Some(PLAN_UNSUPPORTED) => {
            cache.remember(key, CacheState::PlanUnsupported, now);
            Some(PLAN_UNSUPPORTED)
        }
        some => some,
    }
}

/// Turns the branch's protection facts, together with what the probes themselves
/// reported, into the gate's answer: a real (if insufficient) fact names the
/// missing rule, an unsupported plan is told that permanently, and a probe that
/// simply failed to answer is told to retry.
fn classify(
    facts: ProtectionFacts,
    mode: ProtectionMode,
    probed: bool,
    plan_unsupported: bool,
) -> Option<&'static str> {
    match facts.unmet(mode) {
        None => None,
        Some(reason) if probed => Some(reason),
        Some(_) if plan_unsupported => Some(PLAN_UNSUPPORTED),
        Some(_) => Some(PROBE_UNAVAILABLE),
    }
}

fn github_slug(entry: &LedgerEntry, registry: &Registry) -> Option<String> {
    registry
        .repos
        .iter()
        .find(|repo| repo.path.to_string_lossy() == entry.repo)
        .and_then(|repo| repo.remote_url.as_deref())
        .and_then(canonical_repo)
        .and_then(|key| key.strip_prefix("github:").map(str::to_owned))
}

/// What a single `gh api` probe reported.
enum Probe {
    /// The endpoint answered with a body.
    Answered(Value),
    /// The endpoint refused with `403`, GitHub's shape for "this repository's plan
    /// does not include this feature" on the branch-protection endpoints — distinct
    /// from a scoped-permission or rate-limit refusal, which return other statuses.
    PlanUnsupported,
    /// The probe did not answer at all: a timeout, a network failure, or a status
    /// this probe does not interpret. Left for the caller to retry uncached.
    Unavailable,
}

fn api(repo: &str, endpoint: &str) -> Probe {
    let mut command = Command::new("gh");
    command.current_dir(repo).args(["api", endpoint]);
    let Ok(output) = run_timeout(command, COMMAND_TIMEOUT) else {
        return Probe::Unavailable;
    };
    if output.status.success() {
        return serde_json::from_slice(&output.stdout)
            .map(Probe::Answered)
            .unwrap_or(Probe::Unavailable);
    }
    match http_status_from_stderr(&output.stderr) {
        Some(403) => Probe::PlanUnsupported,
        _ => Probe::Unavailable,
    }
}

/// Picks the status code out of `gh api`'s failure summary, e.g. `gh: Upgrade to
/// GitHub Pro or make this repository public to enable this feature. (HTTP 403)`.
/// `gh` writes this line to stderr on every non-2xx response; on failure it is the
/// only place the status code survives (`--include` would carry it on stdout too,
/// but at the cost of parsing headers back out of every successful response).
fn http_status_from_stderr(stderr: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(stderr).ok()?;
    let after_marker = text.rsplit_once("(HTTP ")?.1;
    after_marker.split(')').next()?.trim().parse().ok()
}

#[cfg(test)]
#[path = "protection_tests.rs"]
mod tests;
