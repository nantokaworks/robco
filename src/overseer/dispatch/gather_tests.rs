use super::*;

fn workspace(kind: &str) -> dropr::DroprWorkspace {
    dropr::DroprWorkspace {
        kind: kind.into(),
        id: "github:owner/repo".into(),
        name: "repo".into(),
        repo_url: "https://github.com/owner/repo".into(),
    }
}

#[test]
fn an_unmaterialised_workspace_is_skipped_and_logged_once_per_run() {
    // The dropr:rC8ZxtZT913zsmYfnOFhs loop: a virtual workspace 404s on every
    // ready fetch, so the repo must be skipped before fetching, with exactly
    // one decision entry for the whole daemon run rather than one per tick.
    let mut logged = BTreeSet::new();
    let mut captured = None;
    let skipped = skip_unmaterialised("/repo", &workspace("virtual"), &mut logged, |entry| {
        captured = Some(entry.clone());
        Ok(())
    })
    .unwrap();
    assert!(skipped);
    let entry = captured.unwrap();
    assert_eq!(entry.kind, DecisionKind::Skip);
    assert_eq!(entry.reason, "workspace_not_materialised");
    assert_eq!(entry.repo.as_deref(), Some("/repo"));

    // Every later tick: still skipped, but silently.
    for _ in 0..3 {
        let skipped = skip_unmaterialised("/repo", &workspace("virtual"), &mut logged, |_| {
            panic!("a repeated tick must not log again")
        })
        .unwrap();
        assert!(skipped);
    }
}

#[test]
fn materialising_the_workspace_resumes_dispatch_and_rearms_the_log() {
    // The overlay reloads every pass, so the flip to `materialised` must be
    // enough on its own — no daemon restart — and a workspace that later
    // reverts to virtual gets one fresh decision, not silence.
    let mut logged = BTreeSet::from(["/repo".to_string()]);
    let skipped = skip_unmaterialised("/repo", &workspace("materialised"), &mut logged, |_| {
        panic!("a materialised workspace must not log a skip")
    })
    .unwrap();
    assert!(!skipped);

    let mut captured = None;
    let skipped = skip_unmaterialised("/repo", &workspace("virtual"), &mut logged, |entry| {
        captured = Some(entry.clone());
        Ok(())
    })
    .unwrap();
    assert!(skipped);
    assert_eq!(captured.unwrap().reason, "workspace_not_materialised");
}

#[test]
fn repo_skip_emits_skip_decision() {
    let mut captured = None;
    log_repo_skip("/repo", "repo_path_missing", |entry| {
        captured = Some(entry.clone());
        Ok(())
    })
    .unwrap();
    let entry = captured.unwrap();
    assert_eq!(entry.kind, DecisionKind::Skip);
    assert_eq!(entry.reason, "repo_path_missing");
    assert_eq!(entry.repo.as_deref(), Some("/repo"));
}

#[test]
fn fetch_failure_emits_skip_decision() {
    let mut captured = None;
    log_ready_failure(
        "/repo",
        "workspace-1",
        dropr::ReadyDispatchError::Parse,
        |entry| {
            captured = Some(entry.clone());
            Ok(())
        },
    )
    .unwrap();
    let entry = captured.unwrap();
    assert_eq!(entry.kind, DecisionKind::Skip);
    assert_eq!(entry.reason, "ready_parse_failed:workspace-1");
    assert_eq!(entry.repo.as_deref(), Some("/repo"));
}
