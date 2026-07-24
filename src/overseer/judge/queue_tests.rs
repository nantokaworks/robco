//! Queue behaviour: budgets, caches, and what happens to a verdict whose
//! question changed underneath it.

use super::{queue::test_queue, result, tests::candidate};
use crate::config::Config;

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
