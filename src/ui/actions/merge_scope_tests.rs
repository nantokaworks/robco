//! Merges are serialised per repository, never across repositories. These
//! tests cover that boundary; the single-repository guards live in the parent
//! test module, whose fixtures they share.

use super::*;

#[test]
fn merge_in_another_repository_is_not_blocked() {
    let mut app = test_app();
    app.registry.repos = vec![
        repo("/repo-a", vec![agent("a")]),
        repo("/repo-b", vec![agent("b")]),
    ];
    install_job(&mut app, "/repo-a", "a");

    // The spawned worker fails immediately on the non-existent repository
    // path; only the bookkeeping matters here.
    app.start_merge(1, 0);

    assert_eq!(
        app.merge_job(&PathBuf::from("/repo-a")).unwrap().agent_id,
        "a"
    );
    assert_eq!(
        app.merge_job(&PathBuf::from("/repo-b")).unwrap().agent_id,
        "b"
    );
    assert!(app.message.is_none());
}

#[test]
fn merge_command_is_not_rejected_for_another_repository() {
    let mut app = test_app();
    app.registry.repos = vec![
        repo("/repo-a", vec![agent("a")]),
        repo("/repo-b", vec![agent("b")]),
    ];
    install_job(&mut app, "/repo-a", "a");
    select(&mut app, |selection| {
        matches!(selection, Selection::Agent { repo: 1, .. })
    });

    app.merge_selected();

    // The command proceeds past the guard; it still fails later because the
    // repository path does not exist, which is not what this test is about.
    assert!(
        app.message
            .as_ref()
            .is_none_or(|(message, _)| !message.contains("already in progress"))
    );
}

#[test]
fn drain_advances_every_in_flight_merge() {
    let mut app = test_app();
    app.registry.repos = vec![
        repo("/repo-a", vec![agent("a")]),
        repo("/repo-b", vec![agent("b")]),
    ];
    // Senders must outlive the drain, or the receivers report a disconnected
    // worker and the jobs finish instead of advancing a step.
    let _senders: Vec<_> = [("/repo-a", "a"), ("/repo-b", "b")]
        .into_iter()
        .map(|(repo_path, agent_id)| {
            let (sender, receiver) = mpsc::channel();
            sender.send(MergeEvent::Step(PULLING_MAIN)).unwrap();
            app.merge_jobs.insert(
                repo_path.into(),
                MergeJob {
                    agent_id: agent_id.into(),
                    branch: format!("feature/{agent_id}"),
                    step: MERGING_PR,
                    receiver,
                },
            );
            sender
        })
        .collect();

    app.drain_merge_events().unwrap();

    for repo_path in ["/repo-a", "/repo-b"] {
        assert_eq!(
            app.merge_job(&PathBuf::from(repo_path)).unwrap().step,
            PULLING_MAIN
        );
    }
}

#[test]
fn each_repository_keeps_its_own_merge_outcome() {
    let mut app = test_app();
    app.registry.repos = vec![
        repo("/repo-a", vec![agent("a")]),
        repo("/repo-b", vec![agent("b")]),
    ];
    install_job(&mut app, "/repo-a", "a");
    install_job(&mut app, "/repo-b", "b");

    app.finish_merge_with(
        &PathBuf::from("/repo-a"),
        Err("a failed".into()),
        |_| Ok(()),
    )
    .unwrap();
    app.finish_merge_with(
        &PathBuf::from("/repo-b"),
        Err("b failed".into()),
        |_| Ok(()),
    )
    .unwrap();

    assert_eq!(
        outcome_of(&app, "/repo-a").unwrap().result,
        Err("a failed".into())
    );
    assert_eq!(
        outcome_of(&app, "/repo-b").unwrap().result,
        Err("b failed".into())
    );
    assert!(app.merge_jobs.is_empty());
}

#[test]
fn dismissal_leaves_other_repositories_outcomes_alone() {
    let mut app = test_app();
    app.registry.repos = vec![
        repo("/repo-a", vec![agent("a")]),
        repo("/repo-b", vec![agent("b")]),
    ];
    for (repo_path, agent_id) in [("/repo-a", "a"), ("/repo-b", "b")] {
        install_outcome(
            &mut app,
            repo_path,
            MergeOutcome {
                repo_path: repo_path.into(),
                agent_id: agent_id.into(),
                branch: format!("feature/{agent_id}"),
                result: Err("failed detail".into()),
            },
        );
    }
    select(&mut app, |selection| {
        matches!(selection, Selection::Repo(0))
    });

    assert!(app.dismiss_merge_outcome());
    assert!(outcome_of(&app, "/repo-a").is_none());
    assert!(outcome_of(&app, "/repo-b").is_some());
}
