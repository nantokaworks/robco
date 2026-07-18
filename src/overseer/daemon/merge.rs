use std::{
    collections::HashMap,
    process::Command,
    time::{Duration, Instant},
};

use serde_json::Value;

use super::COMMAND_TIMEOUT;
use crate::{
    Result,
    config::Config,
    dropr::canonical_repo,
    overseer::{
        exec::run_timeout,
        ledger::{Ledger, LedgerEntry, LedgerPhase},
        logging::{self, DecisionEntry, DecisionKind},
    },
    registry::Registry,
};

const PROTECTION_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub(super) struct ProtectionCache(HashMap<String, Instant>);

impl ProtectionCache {
    fn verified(&self, repo: &str, now: Instant) -> bool {
        self.0
            .get(repo)
            .is_some_and(|verified| now.saturating_duration_since(*verified) < PROTECTION_CACHE_TTL)
    }

    fn remember_verified(&mut self, repo: String, now: Instant) {
        self.0.insert(repo, now);
    }

    fn remember_probe(&mut self, repo: &str, now: Instant, result: Option<bool>) {
        if result == Some(true) {
            self.remember_verified(repo.to_owned(), now);
        }
    }
}

pub(super) fn auto_merge_pass(
    config: &Config,
    ledger: &mut Ledger,
    cache: &mut ProtectionCache,
) -> Result<()> {
    if !config.overseer.auto_merge {
        return Ok(());
    }
    let registry = Registry::load()?;
    for entry in ledger
        .entries
        .iter_mut()
        .filter(|entry| entry.phase == LedgerPhase::PrOpened)
    {
        if !protection_verified(entry, &registry, cache)? {
            log(entry, DecisionKind::Hold, "unprotected")?;
            continue;
        }
        let Some(url) = entry.pr_url.clone() else {
            log(entry, DecisionKind::Hold, "missing_pr_url")?;
            continue;
        };
        let mut view = Command::new("gh");
        view.current_dir(&entry.repo).args([
            "pr",
            "view",
            &url,
            "--json",
            "state,statusCheckRollup",
        ]);
        let output = match run_timeout(view, COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                log(
                    entry,
                    DecisionKind::Hold,
                    &format!("check_probe_exit:{}", output.status),
                )?;
                continue;
            }
            Err(error) => {
                log(entry, DecisionKind::Hold, &format!("check_probe:{error}"))?;
                continue;
            }
        };
        let value: Value = match serde_json::from_slice(&output.stdout) {
            Ok(value) => value,
            Err(error) => {
                log(entry, DecisionKind::Hold, &format!("check_parse:{error}"))?;
                continue;
            }
        };
        if !checks_green(&value) {
            log(entry, DecisionKind::Hold, "checks_not_green")?;
            continue;
        }
        let strategy = match config.overseer.merge_strategy.as_str() {
            "merge" => "--merge",
            "rebase" => "--rebase",
            _ => "--squash",
        };
        let mut merge = Command::new("gh");
        merge
            .current_dir(&entry.repo)
            .args(["pr", "merge", &url, strategy]);
        match run_timeout(merge, COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                entry.phase = LedgerPhase::Merged;
                ledger.counters.consecutive_failures = 0;
                log(
                    entry,
                    DecisionKind::Merge,
                    strategy.trim_start_matches("--"),
                )?;
            }
            Ok(output) => log(
                entry,
                DecisionKind::Hold,
                &format!("merge_exit:{}", output.status),
            )?,
            Err(error) => log(entry, DecisionKind::Hold, &format!("merge_error:{error}"))?,
        }
    }
    Ok(())
}

fn protection_verified(
    entry: &LedgerEntry,
    registry: &Registry,
    cache: &mut ProtectionCache,
) -> Result<bool> {
    let now = Instant::now();
    if cache.verified(&entry.repo, now) {
        return Ok(true);
    }
    let remote = registry
        .repos
        .iter()
        .find(|repo| repo.path.to_string_lossy() == entry.repo)
        .and_then(|repo| repo.remote_url.as_deref());
    let Some(name) = remote
        .and_then(canonical_repo)
        .and_then(|key| key.strip_prefix("github:").map(str::to_owned))
    else {
        return Ok(false);
    };
    let endpoint = format!("repos/{name}/branches/main/protection");
    let mut command = Command::new("gh");
    command.current_dir(&entry.repo).args(["api", &endpoint]);
    let result = run_timeout(command, COMMAND_TIMEOUT)
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| serde_json::from_slice(&output.stdout).ok())
        .map(|value| protection_allows_merge(&value));
    cache.remember_probe(&entry.repo, now, result);
    Ok(result.unwrap_or(false))
}

pub(crate) fn protection_allows_merge(value: &Value) -> bool {
    value
        .get("required_pull_request_reviews")
        .is_some_and(|value| value.is_object())
        && value.get("required_status_checks").is_some_and(|checks| {
            checks.is_object()
                && (checks
                    .get("contexts")
                    .and_then(Value::as_array)
                    .is_some_and(|items| !items.is_empty())
                    || checks
                        .get("checks")
                        .and_then(Value::as_array)
                        .is_some_and(|items| !items.is_empty()))
        })
}

pub(crate) fn checks_green(value: &Value) -> bool {
    if value.get("state").and_then(Value::as_str) != Some("OPEN") {
        return false;
    }
    let Some(checks) = value.get("statusCheckRollup").and_then(Value::as_array) else {
        return false;
    };
    !checks.is_empty()
        && checks.iter().all(|check| {
            check
                .get("conclusion")
                .or_else(|| check.get("state"))
                .and_then(Value::as_str)
                == Some("SUCCESS")
        })
}

fn log(entry: &LedgerEntry, kind: DecisionKind, reason: &str) -> Result<()> {
    let mut decision = DecisionEntry::new(kind, reason);
    decision.task = Some(entry.task_id.clone());
    decision.repo = Some(entry.repo.clone());
    decision.source = Some("auto_merge".into());
    logging::append(&decision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unprotected_or_missing_status_checks_refuses() {
        assert!(!protection_allows_merge(&json!({"message": "Not Found"})));
        assert!(!protection_allows_merge(
            &json!({"required_pull_request_reviews": {}, "required_status_checks": null})
        ));
        assert!(protection_allows_merge(
            &json!({"required_pull_request_reviews": {}, "required_status_checks": {"contexts": ["test"]}})
        ));
    }

    #[test]
    fn any_non_success_check_holds() {
        assert!(!checks_green(&json!({"state":"OPEN", "statusCheckRollup":[
            {"conclusion":"SUCCESS"}, {"conclusion":"FAILURE"}
        ]})));
        assert!(checks_green(
            &json!({"state":"OPEN", "statusCheckRollup":[{"conclusion":"SUCCESS"}]})
        ));
    }

    #[test]
    fn positive_cache_expires_and_failures_are_not_remembered() {
        let mut cache = ProtectionCache::default();
        let now = Instant::now();
        cache.remember_probe("/repo", now, None);
        cache.remember_probe("/unprotected", now, Some(false));
        assert!(cache.0.is_empty());

        cache.remember_probe("/repo", now, Some(true));
        assert!(cache.verified("/repo", now + PROTECTION_CACHE_TTL / 2));
        assert!(!cache.verified("/repo", now + PROTECTION_CACHE_TTL));
    }
}
