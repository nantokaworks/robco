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
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
    }
}

#[test]
fn a_loosened_gate_is_identifiable_from_the_decision_alone() {
    let entry = entry();
    let merged = serde_json::to_value(gated_decision(
        &entry,
        DecisionKind::Merge,
        "squash",
        ProtectionMode::Off,
    ))
    .unwrap();
    assert_eq!(merged["protection_mode"], "off");
    assert_eq!(merged["reason"], "squash");
    // Decisions the protection gate does not govern stay free of the field.
    let unrelated =
        serde_json::to_value(decision(&entry, DecisionKind::Hold, "checks_not_green")).unwrap();
    assert!(unrelated.get("protection_mode").is_none());
}

#[test]
fn only_a_gated_halt_carries_the_strictness_mode() {
    // Both readers of a halt — the decision log and merge recovery — see the same
    // reason string; only the protection-governed one also names the mode.
    assert_eq!(Halt::hold("checks_not_green").kind, DecisionKind::Hold);
    assert_eq!(
        Halt::escalate("judge_veto:no rollback").kind,
        DecisionKind::Escalate
    );
    let gated = Halt::gated("unprotected:unknown_remote");
    assert_eq!(gated.kind, DecisionKind::Hold);
    assert_eq!(gated.reason, "unprotected:unknown_remote");
}
