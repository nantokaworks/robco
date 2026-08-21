//! A ledger file written by an older build must still load — split out of
//! `ledger_tests.rs` to keep that file under this project's source file
//! size limit.

use super::*;

#[test]
fn missing_ledger_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ledger.json");
    assert_eq!(Ledger::load_from(&path).unwrap(), Ledger::default());
}

#[test]
fn corrupt_ledger_is_preserved_aside() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ledger.json");
    fs::write(&path, "not json").unwrap();
    assert_eq!(Ledger::load_from(&path).unwrap(), Ledger::default());
    assert!(!path.exists());
    assert_eq!(
        fs::read_to_string(path.with_extension("json.corrupt")).unwrap(),
        "not json"
    );
}

/// A ledger written before the barrier existed still loads: the merge that was
/// in flight when the daemon was upgraded must not be turned into a corrupt
/// ledger and a lost board.
#[test]
fn a_ledger_without_the_settling_field_loads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ledger.json");
    fs::write(&path, r#"{"entries":[],"skip_list":[]}"#).unwrap();
    assert_eq!(Ledger::load_from(&path).unwrap(), Ledger::default());
}

#[test]
fn a_ledger_written_before_merge_recovery_still_loads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ledger.json");
    fs::write(
        &path,
        r##"{"entries":[{"task_id":"t","display_id":"#1","repo":"/repo","agent_id":"a",
            "branch":"b","phase":"pr_opened","dispatched_at":"2026-07-01T00:00:00Z",
            "retries":0,"pr_url":null}],"skip_list":[],"counters":{}}"##,
    )
    .unwrap();
    let ledger = Ledger::load_from(&path).unwrap();
    let entry = &ledger.entries[0];
    assert_eq!(entry.branch_updates, 0);
    assert_eq!(entry.merge_recovery, MergeRecovery::default());
    // Nothing recorded when this entry settled, and nothing may be invented:
    // the history view reads the absence as "unknown", not as "just now".
    assert_eq!(entry.settled_at, None);
    // The merge in flight when the daemon was upgraded starts from a full hold
    // budget rather than from a ledger the new field cannot be read out of.
    assert_eq!(entry.merge_hold, MergeHold::default());
    // The file is intact, so the load was a genuine default rather than the
    // corrupt-ledger fallback.
    assert!(path.exists());
}

/// A ledger written before `dropr_task_id` existed loads without crashing,
/// and every such entry answers "no known dropr task" rather than being
/// handed a fake one made up from whatever `task_id` happens to hold — see
/// dropr:531. `task_id` here is the exact agent id (`boq-hQwQ`) that
/// surfaced the defect: dropr correctly refused it as "task not found"
/// because it is not a dropr task id at all.
#[test]
fn a_ledger_written_before_dropr_task_id_still_loads_with_no_known_dropr_task() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ledger.json");
    fs::write(
        &path,
        r##"{"entries":[{"task_id":"boq-hQwQ","display_id":"boq-hQwQ","repo":"/repo",
            "agent_id":"boq-hQwQ","branch":"b","phase":"escalated",
            "dispatched_at":"2026-07-01T00:00:00Z","retries":0,"pr_url":null}],
            "skip_list":[],"counters":{}}"##,
    )
    .unwrap();
    let ledger = Ledger::load_from(&path).unwrap();
    let entry = &ledger.entries[0];
    assert_eq!(entry.dropr_task_id, None);
    // The file is intact, so the load was a genuine default rather than the
    // corrupt-ledger fallback.
    assert!(path.exists());
}
