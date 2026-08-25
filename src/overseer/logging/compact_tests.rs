use super::*;
use crate::overseer::logging::{DecisionKind, append_to, corrupt_line_count_at};

#[test]
fn compact_quarantines_broken_lines_and_keeps_valid_ones_byte_identical() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decisions.jsonl");
    append_to(&path, &DecisionEntry::new(DecisionKind::Skip, "ok-1")).unwrap();
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{not json}\n").unwrap();
    file.write_all(b"\n").unwrap();
    drop(file);
    append_to(&path, &DecisionEntry::new(DecisionKind::Skip, "ok-2")).unwrap();
    let before = fs::read_to_string(&path).unwrap();
    let valid_lines: Vec<&str> = before
        .lines()
        .filter(|line| serde_json::from_str::<DecisionEntry>(line).is_ok())
        .collect();

    let report = compact_at(&path, false).unwrap();

    assert_eq!(report.kept, 2);
    assert_eq!(report.quarantined, 1);
    assert_eq!(corrupt_line_count_at(&path).unwrap(), 0);
    let after = fs::read_to_string(&path).unwrap();
    let kept_lines: Vec<&str> = after
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(kept_lines, valid_lines);
    let sidecar = fs::read_to_string(&report.sidecar_path).unwrap();
    assert_eq!(sidecar, "{not json}\n");
}

#[test]
fn compact_dry_run_reports_counts_without_touching_the_log() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decisions.jsonl");
    append_to(&path, &DecisionEntry::new(DecisionKind::Skip, "ok")).unwrap();
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{broken\n").unwrap();
    drop(file);
    let before = fs::read_to_string(&path).unwrap();

    let report = compact_at(&path, true).unwrap();

    assert_eq!(report.kept, 1);
    assert_eq!(report.quarantined, 1);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert!(!report.sidecar_path.exists());
}

#[test]
fn compact_is_a_noop_when_the_log_has_no_broken_lines() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decisions.jsonl");
    append_to(&path, &DecisionEntry::new(DecisionKind::Skip, "ok")).unwrap();
    let before = fs::read_to_string(&path).unwrap();

    let report = compact_at(&path, false).unwrap();

    assert_eq!(report.kept, 1);
    assert_eq!(report.quarantined, 0);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert!(!report.sidecar_path.exists());
}

#[test]
fn compact_on_a_missing_log_reports_zero_and_creates_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("does-not-exist.jsonl");

    let report = compact_at(&path, false).unwrap();

    assert_eq!(report.kept, 0);
    assert_eq!(report.quarantined, 0);
    assert!(!path.exists());
    assert!(!report.sidecar_path.exists());
}

/// The same guard `append_to` and `corrupt_line_count_at` carry, extended to
/// the compaction rewrite: a future test that reaches `compact_at` with a
/// path under the operator's real home must fail loudly instead of rewriting
/// their live decision log.
#[test]
#[should_panic(expected = "refused to touch")]
fn compact_at_panics_on_a_path_under_the_operators_real_home() {
    let Some(real_home) = dirs::home_dir() else {
        panic!("refused to touch: no real home dir resolved to test the guard against");
    };
    let path = real_home
        .join(".robco-logging-guard-test")
        .join("decisions.jsonl");
    compact_at(&path, false).unwrap();
}

#[test]
fn compact_loses_no_line_appended_concurrently_with_the_run() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decisions.jsonl");
    for index in 0..20 {
        append_to(
            &path,
            &DecisionEntry::new(DecisionKind::Skip, format!("pre-{index}")),
        )
        .unwrap();
    }
    const WRITERS: usize = 4;
    const PER_WRITER: usize = 30;
    let report = std::thread::scope(|scope| {
        for writer in 0..WRITERS {
            let path = path.clone();
            scope.spawn(move || {
                for index in 0..PER_WRITER {
                    append_to(
                        &path,
                        &DecisionEntry::new(DecisionKind::Skip, format!("w{writer}-{index}")),
                    )
                    .unwrap();
                }
            });
        }
        compact_at(&path, false).unwrap()
    });

    let raw = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = raw.lines().filter(|line| !line.trim().is_empty()).collect();
    let reasons: std::collections::HashSet<String> = lines
        .iter()
        .map(|line| serde_json::from_str::<DecisionEntry>(line).unwrap().reason)
        .collect();
    assert_eq!(report.quarantined, 0);
    assert_eq!(reasons.len(), 20 + WRITERS * PER_WRITER);
    for writer in 0..WRITERS {
        for index in 0..PER_WRITER {
            assert!(reasons.contains(&format!("w{writer}-{index}")));
        }
    }
}
