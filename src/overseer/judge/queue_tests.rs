//! Queue behaviour: budgets, caches, and what happens to a verdict whose
//! question changed underneath it.

use super::{
    Request,
    queue::{JudgmentQueue, test_queue},
    result,
    tests::{candidate, merge_request},
};
use crate::{
    config::{Config, Profile},
    overseer::logging::{self, DecisionKind},
};
use std::{path::Path, thread, time::Duration};

#[test]
fn dispatch_budget_keeps_deterministic_order_without_spawning() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.overseer.daily_llm_budget = 1;
    let mut queue = test_queue(temp.path());
    queue.set_llm_calls_today(1);
    let approved = [candidate("a"), candidate("b")];
    assert!(queue.dispatch_advice(&approved).is_none());
    queue.tick(&config).unwrap();
    assert!(!queue.is_active());
    let advice = queue.dispatch_advice(&approved).unwrap();
    assert_eq!(advice.candidate_ids, ["a", "b"]);
    assert!(advice.fail_safe);
}

#[test]
fn a_judgment_the_candidate_set_outran_is_discarded_on_the_record() {
    // Creating one unrelated task changes `dispatch_key`, and the verdict that
    // came back for the old set is then keyed to a question nobody will ask
    // again. Dropping it silently is what made a whole round read as "the
    // Overseer did nothing this pass".
    let temp = tempfile::tempdir().unwrap();
    let mut queue = test_queue(temp.path());
    let before = [candidate("a"), candidate("b")];
    queue.cache_dispatch(
        &before,
        result::DispatchAdvice {
            candidate_ids: vec!["a".into(), "b".into()],
            reason: "both retained".into(),
            fail_safe: false,
        },
    );

    let after = [candidate("a"), candidate("b"), candidate("c")];
    queue.discard_stale_dispatch(&after).unwrap();

    assert_eq!(queue.completed_len(), 0);
    // The superseding pass may not need a judge at all; the discard is recorded
    // either way.
    assert!(queue.dispatch_advice(&after).is_none());
    let logged = crate::overseer::logging::tail_from(&temp.path().join("decisions.jsonl"), 10)
        .unwrap()
        .into_iter()
        .filter(|entry| {
            entry
                .reason
                .starts_with("judgment_discarded:candidate_set_changed:")
        })
        .count();
    assert_eq!(logged, 1);
}

/// A merge case that clears the deterministic gate spends several passes in the
/// queue before its verdict lands. That wait used to write nothing at all, so
/// `decisions.jsonl` carried an entry for every auto-merge outcome except the
/// one an operator was most likely to mistake for a dead daemon.
#[test]
fn a_pull_request_waiting_on_the_judge_is_recorded_once_and_then_by_its_verdict() {
    for (verdict, recorded) in [
        ("allow", DecisionKind::Merge),
        ("veto", DecisionKind::Escalate),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("decisions.jsonl");
        let config = judging_config(temp.path(), verdict);
        let mut queue = test_queue(temp.path());
        let Request::Merge { case, .. } = merge_request() else {
            unreachable!()
        };

        assert!(queue.merge_advice(case.clone()).unwrap().is_none());
        assert!(queue.merge_advice(case.clone()).unwrap().is_none());
        assert_eq!(
            reasons(&log, "judge_pending"),
            1,
            "queued twice, logged once"
        );

        queue.tick(&config).unwrap();
        assert_eq!(
            queue.snapshot().active.as_deref(),
            Some("merge:task-1"),
            "an active judgment must be visible to `robco overseer status`"
        );
        // A pass that finds the judgment still running must not re-log it.
        assert!(queue.merge_advice(case.clone()).unwrap().is_none());
        assert_eq!(reasons(&log, "judge_pending"), 1);

        settle(&mut queue, &config);
        assert_eq!(queue.merge_advice(case).unwrap().unwrap().reason, "judged");
        assert_eq!(reasons(&log, "judge_pending"), 1);
        assert_eq!(
            logging::tail_from(&log, 10)
                .unwrap()
                .iter()
                .filter(|entry| entry.kind == recorded && entry.reason == "judged")
                .count(),
            1,
            "the verdict itself must still be recorded"
        );
    }
}

/// A judge whose whole job is to write `result.json` and exit.
fn judging_config(dir: &Path, verdict: &str) -> Config {
    let script = crate::overseer::session::executable_script(
        dir,
        &format!(r#"printf '{{"outcome":"{verdict}","reason":"judged"}}' > result.json"#),
    );
    Config {
        profiles: vec![Profile {
            name: "claude".into(),
            program: script.to_string_lossy().into(),
            autonomous_args: vec![],
            model: None,
            backend: None,
        }],
        ..Default::default()
    }
}

/// Polls the queue the way the daemon does until the judgment lands.
fn settle(queue: &mut JudgmentQueue, config: &Config) {
    for _ in 0..400 {
        queue.tick(config).unwrap();
        if !queue.is_active() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("judgment never completed");
}

fn reasons(log: &Path, reason: &str) -> usize {
    logging::tail_from(log, 100)
        .unwrap()
        .iter()
        .filter(|entry| entry.reason == reason)
        .count()
}
