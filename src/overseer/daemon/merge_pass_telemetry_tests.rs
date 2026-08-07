use super::*;

#[test]
fn a_recorded_pass_reads_back_exactly() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("merge_pass.json");
    let at = Utc::now();
    record(
        &path,
        at,
        Duration::from_millis(1234),
        3,
        Some(("/repo-a".to_string(), Duration::from_millis(900))),
    )
    .unwrap();

    let loaded = load(&path).expect("just-written telemetry reads back");
    assert_eq!(loaded.duration_ms, 1234);
    assert_eq!(loaded.repos_evaluated, 3);
    assert_eq!(loaded.slowest_repo.as_deref(), Some("/repo-a"));
    assert_eq!(loaded.slowest_repo_ms, 900);
    // The rename leaves nothing behind for the next reader to trip over.
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn a_pass_with_no_repositories_records_no_slowest() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("merge_pass.json");
    record(&path, Utc::now(), Duration::from_millis(5), 0, None).unwrap();

    let loaded = load(&path).unwrap();
    assert_eq!(loaded.repos_evaluated, 0);
    assert_eq!(loaded.slowest_repo, None);
    assert_eq!(loaded.slowest_repo_ms, 0);
}

#[test]
fn a_missing_file_loads_as_none() {
    let temp = tempfile::tempdir().unwrap();
    assert!(load(&temp.path().join("missing.json")).is_none());
}
