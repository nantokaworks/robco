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

/// An escalation or a failure is a question put to an operator, and the answer
/// is very often the merge itself, performed by hand or landed by a follow-up
/// run. The entry has to be able to learn that regardless of which terminal
/// phase it is sitting in — dropr task #335, where seven entries stayed
/// `escalated` or `failed` for weeks after the work they described had merged.
#[test]
fn a_hand_merged_pull_request_reaches_an_escalated_or_failed_entry() {
    for phase in [LedgerPhase::Escalated, LedgerPhase::Failed] {
        let mut settled = ledger();
        settled.entries[0].phase = phase;
        settled.entries[0].pr_url = Some("https://github.test/pull/1".into());
        let observations: Observations = serde_json::from_str(
            r#"{"prs":[{"taskId":"task-131","url":"https://github.test/pull/1","state":"MERGED","statusCheckRollup":[]}]}"#,
        )
        .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 4, 0).unwrap();
        let (merged, actions) = reconcile(&settled, &observations, now, 30);
        assert_eq!(
            merged.entries[0].phase,
            LedgerPhase::Merged,
            "phase {phase:?}"
        );
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
}

/// The #283 shape from dropr task #335: the entry's recorded `pr_url` names a
/// pull request that closed unmerged, while the work actually landed as a
/// different pull request. Once the reconcile probe finds the real merge —
/// matched by task id, the way a branch-based `gh pr list` reports it — the
/// stale URL must be corrected, not just filled in when it was empty.
#[test]
fn a_stale_recorded_pr_url_is_corrected_to_the_pull_request_that_actually_merged() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    escalated.entries[0].pr_url = Some("https://github.test/pull/213".into());
    let observations: Observations = serde_json::from_str(
        r#"{"prs":[{"taskId":"task-131","url":"https://github.test/pull/212","state":"MERGED","statusCheckRollup":[]}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 4, 0).unwrap();
    let (merged, _) = reconcile(&escalated, &observations, now, 30);
    assert_eq!(merged.entries[0].phase, LedgerPhase::Merged);
    assert_eq!(
        merged.entries[0].pr_url.as_deref(),
        Some("https://github.test/pull/212")
    );
}

/// The other two states move nothing, and must do it quietly. A closed pull
/// request means the work did not land, and an open one is what a settled
/// entry looks like for as long as the operator is still deciding — reviving it
/// would put it back in front of a merge gate that already gave up on it, and
/// reporting either as an unknown state would fill the decision log once per poll
/// interval, which is the noise this whole probe was widened to end.
#[test]
fn an_unmerged_pull_request_leaves_a_settled_entry_where_it_is_and_says_nothing() {
    for phase in [LedgerPhase::Escalated, LedgerPhase::Failed] {
        for state in ["CLOSED", "OPEN"] {
            let mut settled = ledger();
            settled.entries[0].phase = phase;
            settled.entries[0].pr_url = Some("https://github.test/pull/1".into());
            let observations: Observations = serde_json::from_str(&format!(
                r#"{{"prs":[{{"taskId":"task-131","url":"https://github.test/pull/1","state":"{state}","statusCheckRollup":[]}}]}}"#,
            ))
            .unwrap();
            let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 4, 0).unwrap();
            let (unchanged, actions) = reconcile(&settled, &observations, now, 30);
            assert_eq!(
                unchanged.entries[0].phase, phase,
                "phase {phase:?} state {state}"
            );
            assert!(actions.is_empty(), "{phase:?}/{state} should say nothing");
        }
    }
}
