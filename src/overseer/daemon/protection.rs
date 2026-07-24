//! Base-branch protection probing for the auto-merge gate.
//!
//! GitHub exposes branch protection through two independent APIs: the classic
//! `branches/{branch}/protection` endpoint and the rulesets endpoint
//! `rules/branches/{branch}`, which returns the rules already merged across every
//! ruleset that targets the branch. A repository protected only by rulesets answers
//! `404 Branch not protected` on the classic endpoint, so probing one endpoint alone
//! reports genuinely protected repositories as unprotected. Both sources are therefore
//! probed and their facts unioned — GitHub enforces them simultaneously.

use std::{
    collections::HashMap,
    process::Command,
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

/// Reason a branch failed the gate, appended to the `unprotected` hold decision so an
/// operator can see which condition is missing without reading source.
pub(super) const NO_PULL_REQUEST_RULE: &str = "no_pull_request_rule";
pub(super) const NO_REQUIRED_STATUS_CHECKS: &str = "no_required_status_checks";
pub(super) const PROBE_UNAVAILABLE: &str = "probe_unavailable";
pub(super) const UNKNOWN_REMOTE: &str = "unknown_remote";

/// Memoises verified (repository, branch, mode) triples. Loosening the mode or moving to
/// another base branch is a different question, so it re-probes rather than reusing an
/// answer given for a stricter one.
#[derive(Default)]
pub(super) struct ProtectionCache(HashMap<String, Instant>);

impl ProtectionCache {
    fn verified(&self, key: &str, now: Instant) -> bool {
        self.0
            .get(key)
            .is_some_and(|verified| now.saturating_duration_since(*verified) < PROTECTION_CACHE_TTL)
    }

    fn remember_verified(&mut self, key: String, now: Instant) {
        self.0.insert(key, now);
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
    cache: &mut ProtectionCache,
    mode: ProtectionMode,
    branch: &str,
) -> Option<&'static str> {
    if mode == ProtectionMode::Off {
        return None;
    }
    let now = Instant::now();
    let key = cache_key(&entry.repo, branch, mode);
    if cache.verified(&key, now) {
        return None;
    }
    let Some(name) = github_slug(entry, registry) else {
        return Some(UNKNOWN_REMOTE);
    };
    let mut facts = ProtectionFacts::default();
    let mut probed = false;
    if let Some(value) = api(
        &entry.repo,
        &format!("repos/{name}/rules/branches/{branch}"),
    ) {
        probed = true;
        facts = facts.union(ruleset_facts(&value));
    }
    // The rules endpoint never reports classic protection, so a branch the rulesets do
    // not already cover still needs the classic probe before it can be refused.
    if facts.unmet(mode).is_some()
        && let Some(value) = api(
            &entry.repo,
            &format!("repos/{name}/branches/{branch}/protection"),
        )
    {
        probed = true;
        facts = facts.union(classic_facts(&value));
    }
    match facts.unmet(mode) {
        None => {
            cache.remember_verified(key, now);
            None
        }
        // A branch that answered no probe at all is unknown, not unprotected; keep it
        // uncached so the next pass retries.
        Some(_) if !probed => Some(PROBE_UNAVAILABLE),
        Some(reason) => Some(reason),
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

fn api(repo: &str, endpoint: &str) -> Option<Value> {
    let mut command = Command::new("gh");
    command.current_dir(repo).args(["api", endpoint]);
    run_timeout(command, COMMAND_TIMEOUT)
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| serde_json::from_slice(&output.stdout).ok())
}

#[cfg(test)]
#[path = "protection_tests.rs"]
mod tests;
