//! The three questions the board review asks, answered deterministically.
//!
//! An identical failure repeating is the signature of a structural fault rather
//! than a transient one — the failure circuit counts failures but never reads
//! them, so three identical `git worktree add ... already exists` spawns look
//! exactly like three unrelated ones until the circuit trips. A stall produces
//! no failures at all: an hour of held merges raises no counter anywhere.
//!
//! These rules run whether or not a reviewer LLM is configured or in budget, so
//! the review pass never depends on a model being reachable to notice the two
//! patterns that went undiagnosed. What the model adds on top is diagnosis, not
//! detection.

use std::collections::BTreeMap;

use crate::overseer::{config::OverseerConfig, logging::DecisionKind};

use super::digest::Digest;

/// Identical occurrences before a pattern counts as structural.
const REPEAT_THRESHOLD: usize = 3;
/// Multiple of `stuck_after_mins` after which a live entry that never reached a
/// terminal phase is reported. The monitor already fails a *silent* worker at
/// one multiple; this rule is for the entry that stays technically healthy and
/// simply never advances.
const STALL_MULTIPLE: i64 = 2;
/// Most findings one review reports, so a pathological log cannot produce an
/// unbounded escalation storm.
const MAX_FINDINGS: usize = 12;

/// One thing the review wants an operator to look at.
///
/// `key` is stable across passes so a condition that persists is escalated once
/// rather than every interval; `reason` is what lands in `decisions.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Finding {
    pub key: String,
    pub reason: String,
}

pub(super) fn detect(digest: &Digest, config: &OverseerConfig) -> Vec<Finding> {
    let mut findings = Vec::new();
    findings.extend(repeats(digest));
    findings.extend(stalls(digest, config));
    findings.extend(circuit(digest));
    findings.truncate(MAX_FINDINGS);
    findings
}

/// Reasons seen `REPEAT_THRESHOLD` times or more, split by whether they record a
/// failure (structural fault) or a hold (nothing is moving).
fn repeats(digest: &Digest) -> Vec<Finding> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for decision in &digest.decisions {
        // Only per-artifact holds and circuit trips carry a diagnosis. Global
        // skips such as `dispatch_disabled` repeat every pass by design and say
        // nothing about the board.
        let considered = match decision.kind {
            DecisionKind::Hold => decision.task.is_some(),
            DecisionKind::CircuitOpen => true,
            _ => false,
        };
        if considered {
            *counts.entry(decision.reason.as_str()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count >= REPEAT_THRESHOLD)
        .map(|(reason, count)| {
            let kind = if is_failure(reason) {
                "repeating_failure"
            } else {
                "repeating_hold"
            };
            Finding {
                key: format!("{kind}:{reason}"),
                reason: format!("{kind}: {count}× {reason}"),
            }
        })
        .collect()
}

/// A reason that names a failure rather than a policy hold. `spawn_failed:` and
/// the `*_failed` family are the texts the dispatch and merge passes write when
/// something broke, as opposed to when a gate declined.
fn is_failure(reason: &str) -> bool {
    reason.starts_with("spawn_failed:") || reason.contains("_failed") || reason.contains("failed:")
}

fn stalls(digest: &Digest, config: &OverseerConfig) -> Vec<Finding> {
    let threshold = i64::try_from(config.stuck_after_mins)
        .unwrap_or(i64::MAX)
        .saturating_mul(STALL_MULTIPLE);
    digest
        .entries
        .iter()
        .filter(|entry| entry.age_mins >= threshold)
        .map(|entry| Finding {
            key: format!("stalled:{}:{}", entry.task, entry.phase),
            reason: format!(
                "stalled: {} has been {} for {}m in {}",
                entry.task, entry.phase, entry.age_mins, entry.repo
            ),
        })
        .collect()
}

fn circuit(digest: &Digest) -> Vec<Finding> {
    let counters = &digest.counters;
    let threshold = counters.failure_circuit_threshold;
    let failures = counters.consecutive_failures;
    if failures == 0 {
        return Vec::new();
    }
    let cause = latest_failure(digest).unwrap_or("no failure reason recorded");
    if failures >= threshold {
        return vec![Finding {
            key: "circuit:open".into(),
            reason: format!(
                "circuit_open: {failures}/{threshold} consecutive failures; latest {cause}"
            ),
        }];
    }
    if failures.saturating_add(1) >= threshold {
        return vec![Finding {
            key: "circuit:risk".into(),
            reason: format!(
                "circuit_at_risk: {failures}/{threshold} consecutive failures; latest {cause}"
            ),
        }];
    }
    Vec::new()
}

fn latest_failure(digest: &Digest) -> Option<&str> {
    digest
        .decisions
        .iter()
        .rev()
        .map(|decision| decision.reason.as_str())
        .find(|reason| is_failure(reason))
}
