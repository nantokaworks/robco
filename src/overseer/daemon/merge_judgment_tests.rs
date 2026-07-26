//! What a verdict does to an entry is only half of it; the other half is what
//! the *next* pass then sees, so every test here plays out a sequence of passes
//! the way `auto_merge_pass` runs them.
//!
//! [`pass`] is that sequence with the two steps a test cannot run stripped out:
//! the `gh pr view` that produces the pull request, and the append to
//! `decisions.jsonl` — a global path this process must not write to. What is
//! left is exactly what the defect lived in: ask the judge, then charge the
//! answer against the entry's hold budget.

use super::*;
use crate::overseer::{
    daemon::{
        merge_hold::{self, HoldPlan},
        pull_request::head_sha,
    },
    judge::{MergeAdvice, MergeCase, MergeJudgment, merge_case, test_queue},
    ledger::LedgerEntry,
};
use serde_json::json;

const PR_URL: &str = "https://github.com/nantokaworks/robco/pull/222";

fn entry() -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#296".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some(PR_URL.into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
    }
}

/// The pull request the deterministic gate cleared: open, green, and small
/// enough that the autonomy envelope lets the judge be asked at all.
fn pull_request() -> Value {
    json!({
        "state": "OPEN",
        "headRefOid": "abc123",
        "title": "Add a language config",
        "body": "one paragraph",
        "files": [{"path": "docs/guide.md"}],
        "additions": 3,
        "deletions": 1,
        "changedFiles": 1,
    })
}

fn case(entry: &LedgerEntry) -> MergeCase {
    merge_case(entry, PR_URL, &pull_request())
}

fn advice(outcome: MergeJudgment, reason: &str) -> MergeAdvice {
    MergeAdvice {
        outcome,
        reason: reason.into(),
        fail_safe: false,
        ignored_fields: Vec::new(),
    }
}

/// What `judge/completion.rs` writes when a judgment session dies without a
/// verdict — the shape the live incident produced, an OAuth session that could
/// not be refreshed and so never wrote `result.json`.
fn fail_safe() -> MergeAdvice {
    MergeAdvice {
        outcome: MergeJudgment::Escalate,
        reason: "judgment fail-safe: session exited without result.json".into(),
        fail_safe: true,
        ignored_fields: Vec::new(),
    }
}

/// One auto-merge pass over an entry the deterministic gate cleared, in the
/// order `auto_merge_pass` runs it. `None` is a pass that recorded nothing —
/// the silence this whole change exists to end.
fn pass(
    entry: &mut LedgerEntry,
    judgments: &mut JudgmentQueue,
    config: &Config,
) -> Option<(HoldPlan, String)> {
    let value = pull_request();
    let head = head_sha(&value).to_owned();
    match judge_allows(entry, PR_URL, &value, config, judgments, 0).unwrap() {
        Judgment::Allow => panic!("the judge never approved this change"),
        // `Outcome::Pending`: the deterministic gate cleared and only the
        // judgment is outstanding, so whatever the entry was held on is
        // forgotten.
        Judgment::Queued => {
            merge_hold::cleared(entry);
            None
        }
        Judgment::Halt(halt) => {
            let plan = merge_hold::charge(entry, &halt, &head, config.overseer.max_merge_holds);
            Some((plan, halt.reason))
        }
    }
}

/// The defect. A judgment session that dies for an infrastructure reason used
/// to be cached against the change fingerprint as if it had refused the diff:
/// the entry escalated, every later pass found the cached verdict, recorded
/// nothing, and the pull request had no exit but a human.
#[test]
fn a_failed_judge_session_is_re_asked_rather_than_cached_as_a_refusal() {
    let temp = tempfile::tempdir().unwrap();
    let mut judgments = test_queue(temp.path());
    let config = Config::default();
    let mut entry = entry();
    judgments.cache_merge(&case(&entry), fail_safe());

    let (plan, reason) = pass(&mut entry, &mut judgments, &config).expect("the pass is recorded");
    assert_eq!(plan, HoldPlan::Record, "the hold budget bounds the wait");
    assert!(reason.starts_with("judge_unavailable:"), "{reason}");
    assert_eq!(
        entry.phase,
        LedgerPhase::PrOpened,
        "a failed session says nothing about the change"
    );
    assert!(
        !judgments.has_terminal_merge(&entry.task_id, entry.pr_url.as_deref()),
        "a fail-safe must not be remembered against the fingerprint"
    );

    // The next pass reaches the same entry through the ordinary phase check and
    // asks again, rather than reading the failure back as a verdict.
    assert!(pass(&mut entry, &mut judgments, &config).is_none());
    assert_eq!(judgments.pending_len(), 1, "the judge is asked again");
}

/// The bound on the re-asks. Each one costs a model session, so a judge that is
/// never coming back has to reach an operator — and the passes spent waiting for
/// the re-asked judgment must not reset the count, which is exactly what
/// clearing the hold used to do.
#[test]
fn a_judge_that_never_answers_escalates_instead_of_re_asking_forever() {
    let temp = tempfile::tempdir().unwrap();
    let mut judgments = test_queue(temp.path());
    let mut config = Config::default();
    config.overseer.max_judge_retries = 2;
    let mut entry = entry();

    // The first failure plus its re-asks: every one of them keeps the entry
    // where the next pass can still reach it.
    for attempt in 0..config.overseer.max_judge_retries {
        judgments.cache_merge(&case(&entry), fail_safe());
        let (_, reason) = pass(&mut entry, &mut judgments, &config).expect("the pass is recorded");
        assert!(reason.starts_with("judge_unavailable:"), "{reason}");
        assert_eq!(entry.phase, LedgerPhase::PrOpened, "attempt {attempt}");
        // The pull request waits for the session this pass queued.
        assert!(pass(&mut entry, &mut judgments, &config).is_none());
    }

    judgments.cache_merge(&case(&entry), fail_safe());
    let (_, reason) = pass(&mut entry, &mut judgments, &config).expect("the pass is recorded");
    assert!(
        reason.starts_with("judge_unavailable_cap_reached:"),
        "{reason}"
    );
    assert!(
        reason.ends_with("session exited without result.json"),
        "the failure is carried"
    );
    assert_eq!(entry.phase, LedgerPhase::Escalated);
}

/// A verdict the judge actually gave hands the count back: the session worked,
/// so whatever failed before it was the transient thing the count survives
/// rather than the broken judge it bounds.
#[test]
fn a_verdict_that_lands_gives_the_re_asks_back() {
    let temp = tempfile::tempdir().unwrap();
    let mut judgments = test_queue(temp.path());
    let config = Config::default();
    let mut entry = entry();

    judgments.cache_merge(&case(&entry), fail_safe());
    pass(&mut entry, &mut judgments, &config);
    assert_eq!(entry.merge_hold.judge_failures, 1);

    judgments.cache_merge(&case(&entry), advice(MergeJudgment::Allow, "reviewed"));
    let value = pull_request();
    assert!(matches!(
        judge_allows(&mut entry, PR_URL, &value, &config, &mut judgments, 0).unwrap(),
        Judgment::Allow
    ));
    assert_eq!(entry.merge_hold.judge_failures, 0);
    assert_eq!(entry.phase, LedgerPhase::PrOpened);
}

/// The other half of the silence. A veto *is* a verdict about the change, so it
/// stays remembered and the entry stays escalated — but the passes that
/// reconsider it and decline to act have to say so, or a pull request Overseer
/// is deliberately holding reads exactly like one the merge pass never reached.
#[test]
fn a_refusal_that_still_stands_is_recorded_rather_than_waited_on() {
    let temp = tempfile::tempdir().unwrap();
    let mut judgments = test_queue(temp.path());
    let config = Config::default();
    let mut entry = entry();
    judgments.cache_merge(&case(&entry), advice(MergeJudgment::Veto, "unsafe"));

    let (_, reason) = pass(&mut entry, &mut judgments, &config).expect("the pass is recorded");
    assert_eq!(reason, "judge_veto:unsafe");
    assert_eq!(entry.phase, LedgerPhase::Escalated);
    // Both halves of what puts an escalated entry back in front of the gate.
    assert!(judgments.has_terminal_merge(&entry.task_id, entry.pr_url.as_deref()));

    let (plan, reason) = pass(&mut entry, &mut judgments, &config).expect("no pass is silent");
    assert_eq!(reason, "judge_verdict_stands");
    assert_eq!(plan, HoldPlan::Record);
    assert_eq!(
        judgments.pending_len(),
        0,
        "a standing refusal is not re-asked"
    );
}
