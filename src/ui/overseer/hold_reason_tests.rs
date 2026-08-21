//! `held_reason` on its own (dropr:529): which phase answers, which reasons
//! stay quiet, and the `checks_waiting` delay.

use chrono::Utc;

use super::*;
use crate::overseer::ledger::{LedgerEntry, MergeHold};

fn ledger(phase: LedgerPhase, reason: Option<&str>, passes: u32) -> Ledger {
    let mut ledger = Ledger::default();
    ledger.entries.push(LedgerEntry {
        task_id: "#529".into(),
        display_id: "#529".into(),
        repo: "nantokaworks/robco".into(),
        agent_id: "worker-1".into(),
        branch: "robco/worker-1".into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: MergeHold {
            reason: reason.map(str::to_string),
            head: Some("f002d389".into()),
            passes,
            escalated: false,
        },
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
    });
    ledger
}

#[test]
fn checks_not_green_earns_a_line_on_the_first_pass() {
    let ledger = ledger(LedgerPhase::PrOpened, Some("checks_not_green"), 1);
    assert_eq!(
        held_reason(&ledger, "worker-1"),
        Some("A required check failed on the pull request.".into())
    );
}

#[test]
fn checks_waiting_stays_quiet_until_the_third_pass() {
    for passes in [0, 1, 2] {
        let ledger = ledger(LedgerPhase::PrOpened, Some("checks_waiting"), passes);
        assert_eq!(held_reason(&ledger, "worker-1"), None, "passes={passes}");
    }
}

#[test]
fn checks_waiting_earns_a_line_once_it_has_survived_long_enough() {
    let ledger = ledger(LedgerPhase::PrOpened, Some("checks_waiting"), 3);
    assert_eq!(
        held_reason(&ledger, "worker-1"),
        Some("The pull request's checks are still running.".into())
    );
}

#[test]
fn a_reason_this_table_has_never_seen_comes_back_verbatim() {
    let ledger = ledger(LedgerPhase::PrOpened, Some("merge_state:draft"), 1);
    assert_eq!(
        held_reason(&ledger, "worker-1"),
        Some("merge_state:draft".into())
    );
}

#[test]
fn no_hold_recorded_says_nothing() {
    let ledger = ledger(LedgerPhase::PrOpened, None, 0);
    assert_eq!(held_reason(&ledger, "worker-1"), None);
}

#[test]
fn a_phase_other_than_pr_opened_says_nothing_even_with_a_reason() {
    for phase in [
        LedgerPhase::Dispatched,
        LedgerPhase::Claimed,
        LedgerPhase::Working,
        LedgerPhase::Merged,
        LedgerPhase::Failed,
        LedgerPhase::Escalated,
    ] {
        let ledger = ledger(phase, Some("checks_not_green"), 5);
        assert_eq!(held_reason(&ledger, "worker-1"), None, "{phase:?}");
    }
}

#[test]
fn an_agent_with_no_ledger_entry_says_nothing() {
    let ledger = ledger(LedgerPhase::PrOpened, Some("checks_not_green"), 1);
    assert_eq!(held_reason(&ledger, "worker-2"), None);
}

/// dropr:534: a dropped merge approval earns a line immediately, unlike
/// `merge_hold.reason` — there is no quiet window for it, because the
/// operator's own `m` keypress is what stopped counting.
#[test]
fn a_dropped_merge_approval_earns_a_line_with_no_quiet_window() {
    let mut ledger = ledger(LedgerPhase::PrOpened, None, 0);
    ledger.entries[0].approval_dropped = Some("merge_approval_dropped:stale_head:abc..def".into());
    assert_eq!(
        held_reason(&ledger, "worker-1"),
        Some(
            "The pull request moved past the approved revision, so the operator's merge \
             approval no longer applies."
                .into()
        )
    );
}

/// dropr:534: a dropped approval outranks a stale, un-cleared
/// `merge_hold.reason` left over from an earlier pass — `take_merge_approval`
/// never touches that field, so without this priority the row could show a
/// condition that no longer describes what actually happened.
#[test]
fn a_dropped_merge_approval_outranks_a_stale_hold_reason() {
    let mut ledger = ledger(LedgerPhase::PrOpened, Some("checks_waiting"), 11);
    ledger.entries[0].approval_dropped = Some("merge_approval_dropped:stale_head:abc..def".into());
    assert_eq!(
        held_reason(&ledger, "worker-1"),
        Some(
            "The pull request moved past the approved revision, so the operator's merge \
             approval no longer applies."
                .into()
        )
    );
}

#[test]
fn a_dropped_merge_approval_says_nothing_once_the_entry_leaves_pr_opened() {
    let mut ledger = ledger(LedgerPhase::Escalated, None, 0);
    ledger.entries[0].approval_dropped = Some("merge_approval_dropped:stale_head:abc..def".into());
    assert_eq!(held_reason(&ledger, "worker-1"), None);
}

/// dropr:530: a handback withheld because the worker's session is busy earns
/// a line — the row is the operator's only way to see it, since the worker
/// itself was never told.
#[test]
fn a_pending_handback_earns_a_line() {
    let mut ledger = ledger(LedgerPhase::PrOpened, None, 0);
    ledger.entries[0].merge_recovery.pending = Some(crate::overseer::ledger::PendingHandback {
        reason: "checks_not_green".into(),
        head: "f002d389".into(),
        base: "base".into(),
        attempts: 1,
    });
    assert_eq!(
        held_reason(&ledger, "worker-1"),
        Some("A recovery handback is waiting for the worker to be idle.".into())
    );
}

/// A pending handback outranks a stale `merge_hold.reason`, the same way a
/// dropped approval does: it is newer, more specific information.
#[test]
fn a_pending_handback_outranks_a_stale_hold_reason() {
    let mut ledger = ledger(LedgerPhase::PrOpened, Some("checks_waiting"), 11);
    ledger.entries[0].merge_recovery.pending = Some(crate::overseer::ledger::PendingHandback {
        reason: "checks_not_green".into(),
        head: "f002d389".into(),
        base: "base".into(),
        attempts: 1,
    });
    assert_eq!(
        held_reason(&ledger, "worker-1"),
        Some("A recovery handback is waiting for the worker to be idle.".into())
    );
}

#[test]
fn a_pending_handback_says_nothing_once_the_entry_leaves_pr_opened() {
    let mut ledger = ledger(LedgerPhase::Escalated, None, 0);
    ledger.entries[0].merge_recovery.pending = Some(crate::overseer::ledger::PendingHandback {
        reason: "checks_not_green".into(),
        head: "f002d389".into(),
        base: "base".into(),
        attempts: 1,
    });
    assert_eq!(held_reason(&ledger, "worker-1"), None);
}
