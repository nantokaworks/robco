//! `select_and_gate`, the candidate-selection and gate half of `!run`
//! (dropr:465) — the part of `run_named` that is testable without the real
//! dropr/registry I/O `gather::gather_candidates` does.

use std::collections::HashMap;

use super::*;
use crate::overseer::dispatch::run::select_and_gate;

#[test]
fn a_task_absent_from_the_ready_feed_is_refused_as_not_ready() {
    let config = OverseerConfig::default();
    let ledger = Ledger::default();
    let candidates = [candidate("/repo")];
    let outcome = select_and_gate(
        &candidates,
        "no-such-task",
        &config,
        &ledger,
        now(),
        &HashMap::new(),
    );
    assert_eq!(
        outcome.unwrap_err(),
        crate::overseer::dispatch::run::RunOutcome::Refused("not_ready".into())
    );
}

#[test]
fn a_skip_listed_task_is_refused_with_the_gates_own_reason() {
    let config = OverseerConfig::default();
    let mut ledger = Ledger::default();
    ledger.skip_list.push("task-1".into());
    let candidates = [candidate("/repo")];
    let outcome = select_and_gate(
        &candidates,
        "task-1",
        &config,
        &ledger,
        now(),
        &HashMap::new(),
    );
    assert_eq!(
        outcome.unwrap_err(),
        crate::overseer::dispatch::run::RunOutcome::Refused("skip_list".into())
    );
}

#[test]
fn a_ready_task_clears_the_gate_whether_dispatch_is_enabled_or_not() {
    // The whole point of `!run`: it must not read `dispatch_enabled` at all,
    // unlike `plan_dispatch`'s own global pre-check.
    for dispatch_enabled in [true, false] {
        let config = OverseerConfig {
            dispatch_enabled,
            ..OverseerConfig::default()
        };
        let ledger = Ledger::default();
        let candidates = [candidate("/repo")];
        let selected = select_and_gate(
            &candidates,
            "task-1",
            &config,
            &ledger,
            now(),
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(selected.task_id, "task-1");
    }
}

#[test]
fn matches_by_display_id_too() {
    let config = OverseerConfig::default();
    let ledger = Ledger::default();
    let candidates = [candidate("/repo")];
    let selected =
        select_and_gate(&candidates, "#1", &config, &ledger, now(), &HashMap::new()).unwrap();
    assert_eq!(selected.display_id, "#1");
}
