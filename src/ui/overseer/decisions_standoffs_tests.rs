use super::*;

#[test]
fn an_external_claim_names_the_holder() {
    let decisions = [decision(
        DecisionKind::Hold,
        "#216",
        "claimed_elsewhere:manual-run",
    )];
    assert_eq!(standoffs(&decisions), ["#216 → manual-run"]);
}

#[test]
fn a_later_dispatch_clears_the_standoff() {
    // The operator's manual run finished and the overseer picked the task
    // up; the frame must stop reporting a stand-off that ended.
    let decisions = [
        decision(DecisionKind::Hold, "#216", "claimed_elsewhere:manual-run"),
        decision(DecisionKind::Dispatch, "#216", "worker spawned"),
    ];
    assert!(standoffs(&decisions).is_empty());
}

#[test]
fn a_repeated_standoff_is_reported_once() {
    let decisions = [
        decision(DecisionKind::Hold, "#216", "claimed_elsewhere:manual-run"),
        decision(DecisionKind::Hold, "#216", "claimed_elsewhere:other-agent"),
    ];
    assert_eq!(standoffs(&decisions), ["#216 → other-agent"]);
}

#[test]
fn unrelated_decisions_are_ignored() {
    let decisions = [
        decision(DecisionKind::Skip, "#216", "daily_limit"),
        DecisionEntry::new(DecisionKind::Hold, "claimed_elsewhere:no-task"),
    ];
    assert!(standoffs(&decisions).is_empty());
}
