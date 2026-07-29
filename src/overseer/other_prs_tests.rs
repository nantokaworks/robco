use super::*;

fn pr(number: u64, state: &str) -> OtherPr {
    OtherPr {
        number,
        title: format!("bump dependency {number}"),
        author: "app/dependabot".into(),
        url: format!("https://example.test/pull/{number}"),
        head_ref_name: format!("dependabot/{number}"),
        mergeable_state: state.into(),
    }
}

#[test]
fn save_load_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("overseer/other_prs.json");
    let mut other_prs = OtherPrs::default();
    other_prs.repos.insert(
        "/repos/nex".into(),
        RepoOtherPrs {
            polled_at: Utc::now(),
            prs: vec![pr(742, "UNSTABLE"), pr(743, "CLEAN")],
        },
    );

    other_prs.save_to(&path).unwrap();
    let serialized = fs::read_to_string(&path).unwrap();
    assert!(serialized.contains("\"UNSTABLE\""));
    assert_eq!(OtherPrs::load_from(&path).unwrap(), other_prs);
}

/// No file yet — the daemon has never run the probe, or this is a fresh
/// install. An empty board, not an error.
#[test]
fn a_missing_file_loads_as_empty() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("does-not-exist/other_prs.json");
    assert_eq!(OtherPrs::load_from(&path).unwrap(), OtherPrs::default());
}

/// A cache, not a ledger: corruption costs a re-list rather than a crash or a
/// moved-aside file the operator has to notice.
#[test]
fn a_corrupt_file_loads_as_empty() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("other_prs.json");
    fs::write(&path, "not json").unwrap();
    assert_eq!(OtherPrs::load_from(&path).unwrap(), OtherPrs::default());
}
