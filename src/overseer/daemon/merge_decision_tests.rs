use super::*;
use crate::overseer::ledger::LedgerPhase;

fn entry() -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
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
    }
}

#[test]
fn a_loosened_gate_is_identifiable_from_the_decision_alone() {
    let mut entry = entry();
    let merged = serde_json::to_value(gated_decision(
        &mut entry,
        DecisionKind::Merge,
        "squash",
        "",
        ProtectionMode::Off,
    ))
    .unwrap();
    assert_eq!(merged["protection_mode"], "off");
    assert_eq!(merged["reason"], "squash");
    // Decisions the protection gate does not govern stay free of the field.
    let unrelated = serde_json::to_value(decision(
        &mut entry,
        DecisionKind::Hold,
        "checks_not_green",
        "",
    ))
    .unwrap();
    assert!(unrelated.get("protection_mode").is_none());
}

#[test]
fn only_a_gated_halt_carries_the_strictness_mode() {
    // Both readers of a halt — the decision log and merge recovery — see the same
    // reason string; only the protection-governed one also names the mode.
    assert_eq!(Halt::hold("checks_not_green").kind, DecisionKind::Hold);
    assert_eq!(
        Halt::escalate("operator_flagged:no rollback").kind,
        DecisionKind::Escalate
    );
    let gated = Halt::gated("unprotected:unknown_remote");
    assert_eq!(gated.kind, DecisionKind::Hold);
    assert_eq!(gated.reason, "unprotected:unknown_remote");
    // A skip is the gate saying there is nothing here to do, which is not the
    // protection gate's business either.
    assert_eq!(Halt::skip("pr_already_merged").kind, DecisionKind::Skip);
}

/// The exit this task exists for: a pull request merged outside the gate used to
/// hold under `checks_waiting` forever, promising a wait for checks that would
/// never report on a pull request that had already landed.
#[test]
fn a_merged_pull_request_ends_the_entry_instead_of_waiting_on_it() {
    let mut entry = entry();
    let halt = concluded(&mut entry, PrConclusion::Merged);
    assert_eq!(halt.kind, DecisionKind::Skip);
    assert_eq!(halt.reason, "pr_already_merged");
    // The merge this entry was waiting for happened, so the monitor cleans its
    // worker up from here exactly as it does for a merge the gate performed.
    assert_eq!(entry.phase, LedgerPhase::Merged);
}

#[test]
fn a_closed_pull_request_stays_an_operators_problem() {
    let mut entry = entry();
    let halt = concluded(&mut entry, PrConclusion::Closed);
    assert_eq!(halt.kind, DecisionKind::Escalate);
    assert_eq!(halt.reason, "pr_closed_unmerged");
    // Nothing landed and no worker can make it land — reopening is a human act —
    // so the entry stays where `robco`'s inbox shows it to one.
    assert_eq!(entry.phase, LedgerPhase::Escalated);
}

/// `decision` is the single place every merge-gate `Escalate` passes through
/// on its way to `decisions.jsonl`, so this is where the terminal/transient/
/// dedup-by-(reason, head) tag from `merge_escalation` has to land — see
/// dropr:374 and dropr:414.
#[test]
fn an_escalate_decision_is_tagged_by_the_merge_escalation_vocabulary() {
    let mut entry = entry();

    // Terminal: a closed pull request notifies immediately.
    let terminal = decision(
        &mut entry,
        DecisionKind::Escalate,
        "pr_closed_unmerged",
        "abc",
    );
    assert_eq!(terminal.escalation_notify, Some(true));

    // Transient: the hold cap alone is spent, but the recheck loop may still
    // resolve this on its own, so it stays quiet for now.
    let transient = decision(
        &mut entry,
        DecisionKind::Escalate,
        "merge_hold_cap_reached:merge_state:dirty",
        "abc",
    );
    assert_eq!(transient.escalation_notify, Some(false));

    // Outside the vocabulary entirely — an unrecognised custom reason, say — still notifies the
    // first time it is seen for this (reason, head) pair...
    let unclassified = decision(
        &mut entry,
        DecisionKind::Escalate,
        "operator_flagged:no rollback",
        "abc",
    );
    assert_eq!(unclassified.escalation_notify, Some(true));

    // ...but a repeat with the identical reason and head is the same
    // unresolved condition already reported, so it is suppressed (dropr:414).
    let repeat = decision(
        &mut entry,
        DecisionKind::Escalate,
        "operator_flagged:no rollback",
        "abc",
    );
    assert_eq!(repeat.escalation_notify, Some(false));

    // A new push (changed head) is a genuinely new condition and notifies again.
    let new_head = decision(
        &mut entry,
        DecisionKind::Escalate,
        "operator_flagged:no rollback",
        "def",
    );
    assert_eq!(new_head.escalation_notify, Some(true));

    // Only `Escalate` decisions carry the tag at all.
    let hold = decision(&mut entry, DecisionKind::Hold, "pr_closed_unmerged", "abc");
    assert_eq!(hold.escalation_notify, None);
}
