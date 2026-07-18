use super::MergeCase;
use crate::{
    config::Config,
    overseer::{
        autonomy::{ChangeFacts, Decision, merge_envelope_decision},
        ledger::LedgerEntry,
    },
};
use serde_json::Value;

pub(crate) fn judgment_after_gate<T>(
    protection_verified: bool,
    checks_green: bool,
    facts: &ChangeFacts,
    config: &Config,
    judge: impl FnOnce() -> T,
) -> Option<T> {
    matches!(
        merge_envelope_decision(protection_verified, checks_green, facts, &config.overseer),
        Decision::Auto
    )
    .then(judge)
}

pub(crate) fn change_facts(
    value: &Value,
    consecutive_failures: u32,
    llm_calls_today: u32,
) -> ChangeFacts {
    let files = value.get("files").and_then(Value::as_array);
    let additions = value.get("additions").and_then(Value::as_u64);
    let deletions = value.get("deletions").and_then(Value::as_u64);
    let changed = value.get("changedFiles").and_then(Value::as_u64);
    let paths = files
        .into_iter()
        .flatten()
        .filter_map(|file| file.get("path").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let files_known = files.is_some_and(|items| {
        items
            .iter()
            .all(|file| file.get("path").and_then(Value::as_str).is_some())
            && changed.is_some_and(|count| count == paths.len() as u64)
    });
    let only_docs_or_tests = !paths.is_empty()
        && paths.iter().all(|path| {
            path.starts_with("docs/")
                || path.starts_with("tests/")
                || path.contains("/tests/")
                || path.ends_with(".md")
                || path.ends_with("_test.rs")
                || path.ends_with("_tests.rs")
        });
    let contains = |needles: &[&str]| {
        paths
            .iter()
            .any(|path| needles.iter().any(|needle| path.contains(needle)))
    };
    ChangeFacts {
        facts_known: additions.is_some() && deletions.is_some() && changed.is_some() && files_known,
        files_changed: changed.unwrap_or(0).min(u32::MAX as u64) as u32,
        lines_changed: additions
            .unwrap_or(0)
            .saturating_add(deletions.unwrap_or(0))
            .min(u32::MAX as u64) as u32,
        only_docs_or_tests,
        touches_security: contains(&["security", "auth", "permission", "secret"]),
        touches_dependencies: contains(&["Cargo.toml", "Cargo.lock", "package.json", "lockfile"]),
        touches_prod_or_ci: contains(&[".github/", "Dockerfile", "deploy", "production"]),
        consecutive_failures,
        llm_calls_today,
        ..ChangeFacts::default()
    }
}

pub(crate) fn merge_case(entry: &LedgerEntry, url: &str, value: &Value) -> MergeCase {
    let deletions = value
        .get("deletions")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    let additions = value
        .get("additions")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    MergeCase {
        task_id: entry.task_id.clone(),
        repo: entry.repo.clone(),
        pr_url: url.to_owned(),
        head_sha: value
            .get("headRefOid")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        title: value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        body: value
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        files: value
            .get("files")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|file| file.get("path").and_then(Value::as_str).map(str::to_owned))
            .collect(),
        additions,
        deletions,
    }
}
