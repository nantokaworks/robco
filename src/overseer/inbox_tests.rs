use super::*;

fn report(kind: ReportKind) -> InboxReport {
    InboxReport {
        at: "2026-07-16T00:00:00Z".parse().unwrap(),
        agent_id: "worker-1".into(),
        kind,
        task_id: Some("130".into()),
        pr_url: None,
        reason: None,
    }
}

#[test]
fn rotation_keeps_only_an_unconsumed_tail() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inbox.jsonl");
    append_report_to(&path, &report(ReportKind::Done)).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"partial")
        .unwrap();

    let mut reader = InboxReader::from_path(&path).unwrap();
    reader.rotation_threshold = 1;
    assert_eq!(reader.read_new().unwrap(), vec![report(ReportKind::Done)]);
    assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
    reader.commit().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "partial");
}

#[test]
fn partial_line_is_retried_after_it_is_completed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inbox.jsonl");
    let expected = report(ReportKind::TurnDone);
    let line = serde_json::to_vec(&expected).unwrap();
    let split = line.len() / 2;
    fs::write(&path, &line[..split]).unwrap();

    let mut reader = InboxReader::from_path(&path).unwrap();
    assert!(reader.read_new().unwrap().is_empty());
    assert_eq!(reader.offset, 0);
    assert_eq!(load_offset(&path.with_extension("offset")).unwrap(), 0);

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&line[split..]).unwrap();
    file.write_all(b"\n").unwrap();
    assert_eq!(reader.read_new().unwrap(), vec![expected]);
    reader.commit().unwrap();
}

#[test]
fn offset_persistence_atomically_replaces_the_old_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inbox.offset");
    let old_path = temp.path().join("old.offset");
    persist_offset(&path, 17).unwrap();
    fs::hard_link(&path, &old_path).unwrap();

    persist_offset(&path, 42).unwrap();

    assert_eq!(fs::read_to_string(path).unwrap(), "42");
    assert_eq!(fs::read_to_string(old_path).unwrap(), "17");
    assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
}

#[test]
fn stale_offset_after_interrupted_rotation_reads_tail_exactly_once() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inbox.jsonl");
    // Crash state between rotation's two writes: the inbox was already
    // replaced by its unconsumed tail, but the offset reset never landed,
    // so the offset file still holds a value beyond the new file's length.
    let unconsumed = report(ReportKind::Blocked);
    append_report_to(&path, &unconsumed).unwrap();
    persist_offset(&path.with_extension("offset"), 10_000).unwrap();

    let mut reader = InboxReader::from_path(&path).unwrap();
    assert_eq!(reader.read_new().unwrap(), vec![unconsumed.clone()]);
    assert_eq!(reader.read_new().unwrap(), vec![unconsumed]);
    reader.commit().unwrap();
    assert!(reader.read_new().unwrap().is_empty());

    // Rotation still converges afterwards.
    reader.rotation_threshold = 1;
    append_report_to(&path, &report(ReportKind::Done)).unwrap();
    assert_eq!(reader.read_new().unwrap(), vec![report(ReportKind::Done)]);
    reader.commit().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "");
    assert_eq!(load_offset(&path.with_extension("offset")).unwrap(), 0);
}

#[test]
fn uncommitted_reports_are_reread_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inbox.jsonl");
    append_report_to(&path, &report(ReportKind::Done)).unwrap();

    assert_eq!(
        InboxReader::from_path(&path).unwrap().read_new().unwrap(),
        vec![report(ReportKind::Done)]
    );
    let mut restarted = InboxReader::from_path(&path).unwrap();
    assert_eq!(
        restarted.read_new().unwrap(),
        vec![report(ReportKind::Done)]
    );
    restarted.commit().unwrap();
    assert!(
        InboxReader::from_path(&path)
            .unwrap()
            .read_new()
            .unwrap()
            .is_empty()
    );
}
