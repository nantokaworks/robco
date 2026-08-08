//! What the review pass does over time: when it runs, what it records, and what
//! it refuses to accept back from a model.

use super::tests::{ledger_with, live_entry, now};
use super::*;
use crate::overseer::{
    ledger::LedgerPhase,
    monitor::{BranchObservation, Observations},
};

fn pass(temp: &std::path::Path) -> ReviewPass {
    ReviewPass::new(temp.join("review"), temp.join("decisions.jsonl")).unwrap()
}

/// Shorthand for the common case: a tick with no branch activity observed.
/// Tests that care about branch activity call `review.tick` directly.
fn tick(
    review: &mut ReviewPass,
    config: &Config,
    ledger: &Ledger,
    now: DateTime<Utc>,
) -> Result<()> {
    review.tick(config, ledger, &Observations::default(), now)
}

fn config_with_review(profile: Option<&str>) -> Config {
    let mut config = Config::default();
    config.overseer.review_profile = profile.map(str::to_owned);
    // Zero budget stops before any model process is launched while leaving the
    // deterministic stage — the part under test — running.
    config.overseer.daily_review_budget = 0;
    config
}

fn logged(path: &std::path::Path) -> Vec<DecisionEntry> {
    logging::tail_from(path, 100).unwrap()
}

/// The detection the default configuration used to skip entirely: with no
/// reviewer profile the whole pass returned before the digest was built, so the
/// two rules written for the incidents that went undiagnosed had never run.
#[test]
fn the_deterministic_findings_run_without_a_reviewer_model() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("decisions.jsonl");
    let config = Config::default();
    assert!(config.overseer.review_profile.is_none());
    for _ in 0..3 {
        let mut held = DecisionEntry::new(DecisionKind::Hold, "checks_waiting");
        held.task = Some("#204".into());
        held.source = Some("auto_merge".into());
        logging::append_to(&log, &held).unwrap();
    }
    let stalled = ledger_with(vec![live_entry(
        "#204",
        LedgerPhase::PrOpened,
        now() - chrono::Duration::minutes(600),
    )]);
    let mut review = pass(temp.path());

    tick(&mut review, &config, &stalled, now()).unwrap();

    let escalated = |entries: Vec<DecisionEntry>| {
        entries
            .into_iter()
            .filter(|entry| entry.kind == DecisionKind::Escalate)
            .collect::<Vec<_>>()
    };
    let found = escalated(logged(&log));
    assert!(found.iter().any(|entry| {
        entry
            .reason
            .starts_with("repeating_hold: 3× checks_waiting")
    }));
    assert!(
        found
            .iter()
            .any(|entry| entry.reason.starts_with("stalled: #204"))
    );
    assert!(
        found
            .iter()
            .all(|entry| entry.source.as_deref() == Some(SOURCE))
    );
    // A profile is what buys the model stage, so without one nothing is spawned,
    // nothing is charged, and no budget notice is written.
    assert!(review.active.is_none());
    assert_eq!(review.calls_today(), 0);
    assert!(
        !logged(&log)
            .iter()
            .any(|entry| entry.reason == "review_budget_exhausted")
    );

    // Both conditions still hold an interval later, and both were already
    // reported: a standing problem escalates once, not once per interval.
    tick(
        &mut review,
        &config,
        &stalled,
        now() + chrono::Duration::minutes(60),
    )
    .unwrap();
    assert_eq!(escalated(logged(&log)).len(), found.len());
}

/// The nex #528 shape end to end: a RUN worker sits `claimed` for hours by
/// design, and `tick` must read the fresh commit on its branch rather than
/// escalate a healthy worker.
#[test]
fn a_worker_with_fresh_branch_activity_does_not_escalate_through_tick() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("decisions.jsonl");
    let config = Config::default();
    let dispatched_at = now() - chrono::Duration::minutes(180);
    let live = ledger_with(vec![live_entry(
        "#528",
        LedgerPhase::Claimed,
        dispatched_at,
    )]);
    let observations = Observations {
        branches: vec![BranchObservation {
            task_id: "task-#528".into(),
            latest_commit_at: Some(now() - chrono::Duration::minutes(15)),
        }],
        ..Observations::default()
    };
    let mut review = pass(temp.path());

    review.tick(&config, &live, &observations, now()).unwrap();

    assert!(
        !logged(&log)
            .iter()
            .any(|entry| entry.kind == DecisionKind::Escalate)
    );
}

#[test]
fn a_finding_that_persists_is_escalated_once_per_occurrence() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("decisions.jsonl");
    let config = config_with_review(Some("claude"));
    let stalled = ledger_with(vec![live_entry(
        "#204",
        LedgerPhase::PrOpened,
        now() - chrono::Duration::minutes(600),
    )]);
    let mut review = pass(temp.path());

    tick(&mut review, &config, &stalled, now()).unwrap();
    let escalations = |entries: Vec<DecisionEntry>| {
        entries
            .into_iter()
            .filter(|entry| entry.kind == DecisionKind::Escalate)
            .count()
    };
    assert_eq!(escalations(logged(&log)), 1);

    // Same condition an interval later: still true, already reported.
    let later = now() + chrono::Duration::minutes(60);
    tick(&mut review, &config, &stalled, later).unwrap();
    assert_eq!(escalations(logged(&log)), 1);

    // It clears, then comes back: a new occurrence, reported again.
    tick(
        &mut review,
        &config,
        &Ledger::default(),
        later + chrono::Duration::minutes(60),
    )
    .unwrap();
    tick(
        &mut review,
        &config,
        &stalled,
        later + chrono::Duration::minutes(120),
    )
    .unwrap();
    assert_eq!(escalations(logged(&log)), 2);
}

#[test]
fn the_interval_gates_the_pass_and_survives_a_restart() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("decisions.jsonl");
    let config = config_with_review(Some("claude"));
    let ledger = Ledger::default();
    let mut review = pass(temp.path());

    tick(&mut review, &config, &ledger, now()).unwrap();
    let after_first = logged(&log).len();
    assert!(after_first > 0);

    // Well inside the interval, and again from a freshly loaded pass: a daemon
    // that restarts often must not review on every start-up.
    tick(
        &mut review,
        &config,
        &ledger,
        now() + chrono::Duration::minutes(1),
    )
    .unwrap();
    tick(
        &mut pass(temp.path()),
        &config,
        &ledger,
        now() + chrono::Duration::minutes(2),
    )
    .unwrap();
    assert_eq!(logged(&log).len(), after_first);

    tick(
        &mut pass(temp.path()),
        &config,
        &ledger,
        now()
            + chrono::Duration::minutes(
                i64::try_from(config.overseer.review_interval_mins).unwrap(),
            ),
    )
    .unwrap();
    assert!(logged(&log).len() > after_first);
}

#[test]
fn an_exhausted_budget_still_reports_the_deterministic_findings() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("decisions.jsonl");
    let stalled = ledger_with(vec![live_entry(
        "#204",
        LedgerPhase::PrOpened,
        now() - chrono::Duration::minutes(600),
    )]);

    tick(
        &mut pass(temp.path()),
        &config_with_review(Some("claude")),
        &stalled,
        now(),
    )
    .unwrap();

    let entries = logged(&log);
    assert!(
        entries
            .iter()
            .any(|entry| entry.kind == DecisionKind::Escalate
                && entry.reason.starts_with("stalled: #204"))
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.reason == "review_budget_exhausted")
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.source.as_deref() == Some(SOURCE))
    );
}
