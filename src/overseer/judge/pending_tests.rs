//! What survives a daemon restart, and what a restart is allowed to cost.

use super::{
    MergeCase, Request,
    keys::merge_key,
    pending::PendingQueue,
    queue::test_queue,
    tests::{candidate, merge_request},
};
use crate::config::{Config, Profile};
use std::{fs, path::Path};

fn dispatch(key: &str) -> Request {
    Request::Dispatch {
        key: key.into(),
        approved: vec![candidate("a")],
    }
}

/// Service order is the whole point of a queue, so it has to be what comes back.
#[test]
fn a_saved_queue_comes_back_in_the_same_order() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pending.json");
    let saved = PendingQueue {
        requests: vec![dispatch("first"), merge_request(), dispatch("last")],
    };

    saved.save(&path).unwrap();

    assert_eq!(PendingQueue::load(&path).unwrap(), saved);
}

/// The session that was running died with the daemon, so the question it was
/// asking is owed another run rather than a wait for an answer nobody is
/// writing.
#[test]
fn a_request_that_was_active_is_pending_after_a_restart() {
    let temp = tempfile::tempdir().unwrap();
    let config = sleeping_config(temp.path());
    let mut queue = test_queue(temp.path());
    assert!(queue.dispatch_advice(&[candidate("a")]).is_none());
    queue.tick(&config).unwrap();
    assert!(queue.is_active());
    drop(queue);

    let restarted = test_queue(temp.path());
    assert_eq!(restarted.pending_len(), 1);
}

/// The expensive half of a restart: a session that finished and wrote its
/// verdict just before the daemon died has already been paid for.
#[test]
fn a_verdict_already_on_disk_is_recovered_without_a_session() {
    let temp = tempfile::tempdir().unwrap();
    let case = restarted_with(
        temp.path(),
        br#"{"outcome":"allow","reason":"already judged"}"#,
    );

    let mut queue = test_queue(temp.path());
    assert_eq!(queue.pending_len(), 1, "the question survived the restart");
    // A profile that cannot launch: reaching it at all would mean the stored
    // verdict was ignored and the model budget spent again.
    queue.tick(&unlaunchable_config()).unwrap();

    assert!(!queue.is_active());
    assert_eq!(queue.llm_calls_today(), 0);
    assert_eq!(
        queue.merge_advice(case).unwrap().unwrap().reason,
        "already judged"
    );
}

/// A truncated `result.json` is not an answer. The session loop already refuses
/// to read one mid-write, and recovery has to hold the same line — otherwise a
/// crash mid-write turns into a fail-safe escalation.
#[test]
fn a_half_written_verdict_is_not_recovered() {
    let temp = tempfile::tempdir().unwrap();
    restarted_with(temp.path(), br#"{"outcome":"allow","rea"#);

    let mut queue = test_queue(temp.path());
    queue.tick(&sleeping_config(temp.path())).unwrap();

    assert!(queue.is_active(), "the question must be asked again");
}

/// Recovery is a restart affordance, not a cache. A round this daemon asked
/// itself reaches a session every time — reading back the file it wrote a moment
/// ago would freeze the judge's answer for as long as the question recurred.
#[test]
fn a_case_directory_is_only_read_back_after_a_restart() {
    let temp = tempfile::tempdir().unwrap();
    let case = stored_verdict(temp.path(), br#"{"outcome":"allow","reason":"stale"}"#);

    let mut queue = test_queue(temp.path());
    assert!(queue.merge_advice(case).unwrap().is_none());
    queue.tick(&sleeping_config(temp.path())).unwrap();

    assert!(queue.is_active());
}

/// An unreadable state file costs at most a re-run of the questions it
/// described; refusing to start would cost the board.
#[test]
fn an_unreadable_state_file_loads_as_an_empty_queue() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pending.json");
    assert_eq!(
        PendingQueue::load(&path).unwrap(),
        PendingQueue::default(),
        "absent"
    );
    fs::write(&path, b"{not json").unwrap();
    assert_eq!(
        PendingQueue::load(&path).unwrap(),
        PendingQueue::default(),
        "corrupt"
    );
}

fn sleeping_config(dir: &Path) -> Config {
    profile_config(
        crate::overseer::session::executable_script(dir, "sleep 30")
            .to_string_lossy()
            .into(),
    )
}

fn unlaunchable_config() -> Config {
    profile_config("/nonexistent/judge-must-not-run".into())
}

fn profile_config(program: String) -> Config {
    Config {
        profiles: vec![Profile {
            name: "claude".into(),
            program,
            autonomous_args: vec![],
            model: None,
            backend: None,
        }],
        ..Default::default()
    }
}

/// Writes `verdict` into the case directory of the merge request under test.
fn stored_verdict(root: &Path, verdict: &[u8]) -> MergeCase {
    let Request::Merge { case, .. } = merge_request() else {
        unreachable!()
    };
    let case_dir = root.join("cases").join(merge_key(&case));
    fs::create_dir_all(&case_dir).unwrap();
    fs::write(case_dir.join("result.json"), verdict).unwrap();
    case
}

/// The state a daemon dying mid-judgment leaves behind: the question in the
/// durable queue, and whatever its session had written by then on disk.
fn restarted_with(root: &Path, verdict: &[u8]) -> MergeCase {
    let case = stored_verdict(root, verdict);
    PendingQueue {
        requests: vec![Request::Merge {
            key: merge_key(&case),
            case: case.clone(),
        }],
    }
    .save(&root.join("cases").join("pending.json"))
    .unwrap();
    case
}
