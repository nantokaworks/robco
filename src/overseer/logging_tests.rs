use super::*;

#[test]
fn bounded_tail_skips_malformed_lines() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decisions.jsonl");
    for index in 0..3 {
        append_to(
            &path,
            &DecisionEntry::new(DecisionKind::Skip, index.to_string()),
        )
        .unwrap();
    }
    let entries = tail_from(&path, 2).unwrap();
    assert_eq!(entries[0].reason, "1");
}

#[test]
fn bounded_tail_reads_end_of_large_log() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decisions.jsonl");
    let mut file = File::create(&path).unwrap();
    for _ in 0..(TAIL_WINDOW_BYTES / 8 + 1) {
        file.write_all(b"invalid\n").unwrap();
    }
    drop(file);
    for index in 0..3 {
        append_to(
            &path,
            &DecisionEntry::new(DecisionKind::Skip, index.to_string()),
        )
        .unwrap();
    }

    let entries = tail_from(&path, 2).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].reason, "1");
    assert_eq!(entries[1].reason, "2");
}

#[test]
fn escalation_batch_produces_one_digest_not_one_line_each() {
    let entries = (0..4)
        .map(|index| {
            let mut entry = DecisionEntry::new(DecisionKind::Escalate, format!("reason-{index}"));
            entry.task = Some(format!("task-{index}"));
            entry
        })
        .collect::<Vec<_>>();
    let digest = coalesce_digest(&entries).unwrap();
    assert!(digest.starts_with("4 overseer alert(s):"));
    assert_eq!(digest.lines().count(), 1);
    assert!(digest.contains("+1 more"));
}

#[test]
fn offset_cursor_digests_large_gap_once() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decisions.jsonl");
    let mut cursor = DigestCursor::at_end_of(path.clone()).unwrap();
    let entry = DecisionEntry::new(DecisionKind::Escalate, "same alert");
    for _ in 0..250 {
        append_to(&path, &entry).unwrap();
    }

    let digest = cursor.read_digest().unwrap().unwrap();
    assert!(digest.starts_with("250 overseer alert(s):"));
    assert!(cursor.read_digest().unwrap().is_none());
}

#[test]
fn concurrent_appends_never_shred_a_line() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decisions.jsonl");
    const WRITERS: usize = 8;
    const PER_WRITER: usize = 50;
    std::thread::scope(|scope| {
        for writer in 0..WRITERS {
            let path = path.clone();
            scope.spawn(move || {
                for index in 0..PER_WRITER {
                    append_to(
                        &path,
                        &DecisionEntry::new(DecisionKind::Skip, format!("{writer}-{index}")),
                    )
                    .unwrap();
                }
            });
        }
    });
    let raw = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = raw.lines().collect();
    assert_eq!(lines.len(), WRITERS * PER_WRITER);
    for line in &lines {
        serde_json::from_str::<DecisionEntry>(line)
            .expect("every concurrently appended line must parse");
    }
}

#[test]
fn corrupt_line_count_ignores_valid_lines_and_counts_only_broken_ones() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decisions.jsonl");
    append_to(&path, &DecisionEntry::new(DecisionKind::Skip, "ok")).unwrap();
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{not json\n").unwrap();
    file.write_all(b"\n").unwrap();
    append_to(&path, &DecisionEntry::new(DecisionKind::Skip, "ok-2")).unwrap();

    assert_eq!(corrupt_line_count_at(&path).unwrap(), 1);
}

#[test]
fn corrupt_line_count_is_zero_for_a_missing_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("does-not-exist.jsonl");
    assert_eq!(corrupt_line_count_at(&path).unwrap(), 0);
}

/// Bypasses the `cfg(test)` home redirect on purpose to observe the real
/// path this process would use outside of tests, and checks a unique
/// marker rather than byte length/mtime: an Overseer daemon may be running
/// against this same real home concurrently with the test suite, and a
/// byte-length comparison would false-positive on its unrelated writes.
#[test]
fn append_never_writes_the_operators_real_decision_log() {
    let Some(real_home) = dirs::home_dir() else {
        return;
    };
    let real_path = real_home
        .join(".robco")
        .join("overseer")
        .join("decisions.jsonl");
    const MARKER: &str = "isolation-guard-marker-dropr-VYj8In1jqvunWtAy3OtCo";
    let contains_marker = |path: &Path| {
        fs::read_to_string(path)
            .map(|contents| contents.contains(MARKER))
            .unwrap_or(false)
    };
    assert!(
        !contains_marker(&real_path),
        "guard marker unexpectedly already present in the real decision log"
    );

    append(&DecisionEntry::new(DecisionKind::Skip, MARKER)).unwrap();

    assert!(
        !contains_marker(&real_path),
        "logging::append wrote to the operator's real decision log during cargo test"
    );
}

/// The second line of defense named in dropr:goePffPb7zAkCztR3HCV8: a test
/// that reaches `append_to` with an explicit path under the operator's real
/// home — bypassing `decision_log_path`'s redirect entirely — must fail
/// loudly instead of quietly writing there. The assertion panics before any
/// file I/O runs, so this test never touches the real path it names.
#[test]
#[should_panic(expected = "refused to touch")]
fn append_to_panics_on_a_path_under_the_operators_real_home() {
    let Some(real_home) = dirs::home_dir() else {
        panic!("refused to touch: no real home dir resolved to test the guard against");
    };
    let path = real_home
        .join(".robco-logging-guard-test")
        .join("decisions.jsonl");
    append_to(
        &path,
        &DecisionEntry::new(DecisionKind::Skip, "must never land"),
    )
    .unwrap();
}

/// The same second line of defense as `append_to`, extended to every other
/// function that reads a decision log by an explicit `&Path`: a future test
/// that reaches `corrupt_line_count_at` with a path under the operator's real
/// home must fail loudly instead of silently scanning it.
#[test]
#[should_panic(expected = "refused to touch")]
fn corrupt_line_count_at_panics_on_a_path_under_the_operators_real_home() {
    let Some(real_home) = dirs::home_dir() else {
        panic!("refused to touch: no real home dir resolved to test the guard against");
    };
    let path = real_home
        .join(".robco-logging-guard-test")
        .join("decisions.jsonl");
    corrupt_line_count_at(&path).unwrap();
}

/// Same guard, for the digest cursor's construction path.
#[test]
#[should_panic(expected = "refused to touch")]
fn digest_cursor_at_end_of_panics_on_a_path_under_the_operators_real_home() {
    let Some(real_home) = dirs::home_dir() else {
        panic!("refused to touch: no real home dir resolved to test the guard against");
    };
    let path = real_home
        .join(".robco-logging-guard-test")
        .join("decisions.jsonl");
    DigestCursor::at_end_of(path).unwrap();
}

/// Same guard, for the tail-reading path.
#[test]
#[should_panic(expected = "refused to touch")]
fn tail_from_panics_on_a_path_under_the_operators_real_home() {
    let Some(real_home) = dirs::home_dir() else {
        panic!("refused to touch: no real home dir resolved to test the guard against");
    };
    let path = real_home
        .join(".robco-logging-guard-test")
        .join("decisions.jsonl");
    tail_from(&path, 10).unwrap();
}
