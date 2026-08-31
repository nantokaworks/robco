use super::*;
use crate::overseer::{
    config::OverseerConfig,
    ledger::{LedgerEntry, LedgerPhase},
    monitor::BranchObservation,
};
use chrono::TimeZone;

pub(super) fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 24, 10, 0, 0).unwrap()
}

fn decision(kind: DecisionKind, task: &str, source: &str, reason: &str) -> DecisionEntry {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(task.into());
    entry.source = Some(source.into());
    entry
}

/// The three patterns the 2026-07-24 incident left in `decisions.jsonl`: two
/// spawn failures that each repeated verbatim, and an hour of held merges that
/// raised no failure at all.
fn incident_log() -> Vec<DecisionEntry> {
    let mut entries = Vec::new();
    for _ in 0..3 {
        entries.push(decision(
            DecisionKind::Hold,
            "#786",
            "dispatch",
            "spawn_failed:io error: command timed out",
        ));
    }
    for _ in 0..3 {
        entries.push(decision(
            DecisionKind::Hold,
            "#210",
            "dispatch",
            "spawn_failed:git worktree add failed: fatal: a branch named 'robco/task-210' already exists",
        ));
    }
    for _ in 0..12 {
        entries.push(decision(
            DecisionKind::Hold,
            "#204",
            "auto_merge",
            "unprotected:no_required_checks",
        ));
    }
    entries
}

pub(super) fn ledger_with(entries: Vec<LedgerEntry>) -> Ledger {
    Ledger {
        entries,
        ..Ledger::default()
    }
}

pub(super) fn live_entry(
    display_id: &str,
    phase: LedgerPhase,
    dispatched_at: DateTime<Utc>,
) -> LedgerEntry {
    LedgerEntry {
        task_id: format!("task-{display_id}"),
        dropr_task_id: None,
        display_id: display_id.into(),
        repo: "/repo".into(),
        agent_id: "auto-agent".into(),
        branch: "branch".into(),
        phase,
        dispatched_at,
        settled_at: None,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        escalation_notified_reason: None,
        escalation_notified_head: None,
        worker_escalated: false,
        operator_override: None,
        merge_approval: None,
        pr_facts: None,
        worker_finished_at: None,
        approval_dropped: None,
        branch_update_head: None,
    }
}

fn reasons(decisions: &[DecisionEntry], ledger: &Ledger, config: &OverseerConfig) -> Vec<String> {
    reasons_with_activity(decisions, ledger, &[], config)
}

pub(super) fn reasons_with_activity(
    decisions: &[DecisionEntry],
    ledger: &Ledger,
    branch_activity: &[BranchObservation],
    config: &OverseerConfig,
) -> Vec<String> {
    let digest = digest::build(decisions, ledger, branch_activity, config, now());
    findings::detect(&digest, config)
        .into_iter()
        .map(|finding| finding.reason)
        .collect()
}

#[test]
fn replaying_the_incident_log_names_every_pattern() {
    let config = OverseerConfig::default();
    let found = reasons(&incident_log(), &Ledger::default(), &config);

    let named = |needle: &str| {
        found
            .iter()
            .any(|reason| reason.contains(needle) && reason.starts_with("repeating_"))
    };
    assert!(named("command timed out"), "{found:?}");
    assert!(named("already exists"), "{found:?}");
    assert!(named("unprotected"), "{found:?}");
    // A failure that repeats is structural; a hold that repeats is a stall. The
    // reviewer must not report them as the same thing.
    assert!(
        found
            .iter()
            .any(|reason| reason.starts_with("repeating_failure: 3× spawn_failed:io error")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|reason| reason.starts_with("repeating_hold: 12× unprotected")),
        "{found:?}"
    );
}

#[test]
fn a_transient_failure_is_not_reported_as_structural() {
    let config = OverseerConfig::default();
    let twice = incident_log()
        .into_iter()
        .filter(|entry| entry.reason.contains("command timed out"))
        .take(2)
        .collect::<Vec<_>>();

    assert!(reasons(&twice, &Ledger::default(), &config).is_empty());
}

#[test]
fn an_entry_that_never_advances_is_reported_as_stalled() {
    let config = OverseerConfig::default();
    let ledger = ledger_with(vec![
        live_entry(
            "#204",
            LedgerPhase::PrOpened,
            now() - chrono::Duration::minutes(180),
        ),
        live_entry(
            "#205",
            LedgerPhase::Working,
            now() - chrono::Duration::minutes(5),
        ),
    ]);

    let found = reasons(&[], &ledger, &config);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("stalled: #204 has been pr_opened for 180m"));
}

#[test]
fn the_circuit_is_reported_at_risk_before_it_opens_and_named_after() {
    let config = OverseerConfig {
        failure_circuit_threshold: 3,
        ..OverseerConfig::default()
    };
    let mut ledger = Ledger::default();
    let log = incident_log();

    ledger.counters.consecutive_failures = 2;
    let at_risk = reasons(&log, &ledger, &config);
    assert!(
        at_risk
            .iter()
            .any(|reason| reason.starts_with("circuit_at_risk: 2/3")),
        "{at_risk:?}"
    );

    ledger.counters.consecutive_failures = 3;
    let open = reasons(&log, &ledger, &config);
    // The cause is read out of the log rather than left to the operator to find.
    assert!(
        open.iter()
            .any(|reason| reason.starts_with("circuit_open: 3/3")
                && reason.contains("already exists")),
        "{open:?}"
    );
}

#[test]
fn the_digest_is_bounded_however_long_the_log_is() {
    let config = OverseerConfig::default();
    let mut decisions = (0..5_000)
        .map(|index| {
            decision(
                DecisionKind::Hold,
                &format!("#{index}"),
                "dispatch",
                &format!("reason {index}"),
            )
        })
        .collect::<Vec<_>>();
    decisions.push(decision(
        DecisionKind::Hold,
        "#1",
        "dispatch",
        &"very ".repeat(400),
    ));
    let entries = (0..500)
        .map(|index| {
            live_entry(
                &format!("#{index}"),
                LedgerPhase::Working,
                now() - chrono::Duration::minutes(index),
            )
        })
        .collect();

    let built = digest::build(&decisions, &ledger_with(entries), &[], &config, now());
    assert_eq!(built.decisions.len(), digest::MAX_DECISIONS);
    assert!(built.entries.len() <= 50);
    assert!(
        built
            .decisions
            .iter()
            .all(|entry| entry.reason.chars().count() <= 161)
    );
}

#[test]
fn the_review_never_reads_its_own_escalations_back() {
    // Otherwise one finding restated every interval becomes evidence of a
    // repeating problem, and the reviewer diagnoses itself.
    let config = OverseerConfig::default();
    let mut own = DecisionEntry::new(
        DecisionKind::Escalate,
        "repeating_failure: 3× spawn_failed:x",
    );
    own.task = Some("#1".into());
    own.source = Some(SOURCE.into());

    let found = reasons(
        &[own.clone(), own.clone(), own],
        &Ledger::default(),
        &config,
    );
    assert!(found.is_empty(), "{found:?}");
}
