//! What an observed pull request state does to a ledger entry.
//!
//! Kept beside `monitor_tests.rs` rather than in it: these cover the one
//! observation that reaches an entry the rest of Overseer has already finished
//! with, which is a different subject from the phase transitions a live worker
//! walks through.

use super::{
    Action, LedgerPhase, Observations, reconcile,
    tests::{ledger, replay},
    types::ObservationSnapshot,
};
use chrono::{TimeZone, Utc};

#[test]
fn merged_snapshot_emits_cleanup_once() {
    let line = r#"{"at":"2026-07-16T00:04:00Z","observations":{"prs":[{"taskId":"task-131","url":"https://github.test/pull/1","state":"MERGED","statusCheckRollup":[]}]}}"#;
    let (merged, actions) = replay(&[line]);
    assert_eq!(merged.entries[0].phase, LedgerPhase::Merged);
    assert!(actions.contains(&Action::KillSession {
        agent_id: "worker-1".into()
    }));
    assert!(actions.contains(&Action::RemoveWorktree {
        agent_id: "worker-1".into(),
    }));
    let (_, actions) = reconcile(
        &merged,
        &serde_json::from_str::<ObservationSnapshot>(line)
            .unwrap()
            .observations,
        Utc.with_ymd_and_hms(2026, 7, 16, 0, 5, 0).unwrap(),
        30,
    );
    assert!(actions.is_empty());
}

#[test]
fn merged_entry_reemits_cleanup_while_agent_is_registered() {
    let mut merged = ledger();
    merged.entries[0].phase = LedgerPhase::Merged;
    let observations: Observations =
        serde_json::from_str(r#"{"registered_agents":["worker-1"]}"#).unwrap();
    let (_, actions) = reconcile(
        &merged,
        &observations,
        Utc.with_ymd_and_hms(2026, 7, 16, 0, 5, 0).unwrap(),
        30,
    );
    assert!(actions.contains(&Action::KillSession {
        agent_id: "worker-1".into(),
    }));
    assert!(actions.contains(&Action::RemoveWorktree {
        agent_id: "worker-1".into(),
    }));
    let (_, actions) = reconcile(
        &merged,
        &Observations::default(),
        Utc.with_ymd_and_hms(2026, 7, 16, 0, 6, 0).unwrap(),
        30,
    );
    assert!(actions.is_empty());
}

/// An escalation is a question put to an operator, and the answer is very often
/// the merge itself, performed by hand. The entry has to be able to learn that:
/// while it was excluded from the pull request probe, one hand-merged pull
/// request sat escalated for hours with its worktree and session still up.
#[test]
fn a_hand_merged_pull_request_reaches_an_escalated_entry() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    escalated.entries[0].pr_url = Some("https://github.test/pull/1".into());
    let observations: Observations = serde_json::from_str(
        r#"{"prs":[{"taskId":"task-131","url":"https://github.test/pull/1","state":"MERGED","statusCheckRollup":[]}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 4, 0).unwrap();
    let (merged, actions) = reconcile(&escalated, &observations, now, 30);
    assert_eq!(merged.entries[0].phase, LedgerPhase::Merged);
    // The same pass that learns of the merge also releases the worker: the
    // transition into `merged` is what pushes cleanup, whatever phase it came
    // from.
    assert!(actions.contains(&Action::KillSession {
        agent_id: "worker-1".into(),
    }));
    assert!(actions.contains(&Action::RemoveWorktree {
        agent_id: "worker-1".into(),
    }));
}

/// The other two states move nothing, and must do it quietly. A closed pull
/// request means the work did not land, and an open one is what an escalated
/// entry looks like for as long as the operator is still deciding — reviving it
/// would put it back in front of a merge gate that already gave up on it, and
/// reporting either as an unknown state would fill the decision log once per poll
/// interval, which is the noise this whole probe was widened to end.
#[test]
fn an_unmerged_pull_request_leaves_an_escalated_entry_where_it_is_and_says_nothing() {
    for state in ["CLOSED", "OPEN"] {
        let mut escalated = ledger();
        escalated.entries[0].phase = LedgerPhase::Escalated;
        escalated.entries[0].pr_url = Some("https://github.test/pull/1".into());
        let observations: Observations = serde_json::from_str(&format!(
            r#"{{"prs":[{{"taskId":"task-131","url":"https://github.test/pull/1","state":"{state}","statusCheckRollup":[]}}]}}"#,
        ))
        .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 4, 0).unwrap();
        let (unchanged, actions) = reconcile(&escalated, &observations, now, 30);
        assert_eq!(unchanged.entries[0].phase, LedgerPhase::Escalated);
        assert!(actions.is_empty(), "{state} should say nothing");
    }
}
