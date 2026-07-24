use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    config::Config,
    model::{AgentNode, RepoNode, Selection, Status},
    registry::Registry,
    ui::Mode,
};

fn agent(id: &str) -> AgentNode {
    let now = Local::now();
    AgentNode {
        management: crate::model::ManagementMode::Manual,
        id: id.to_string(),
        parent_agent_id: None,
        title: id.to_string(),
        worktree_path: format!("/worktrees/{id}").into(),
        branch: format!("feature/{id}"),
        base_commit: String::new(),
        program: "codex".to_string(),
        claude_session_id: None,
        profile: None,
        tmux_session: format!("robco_{id}"),
        created_at: now,
        updated_at: now,
        status: Status::Running,
        worktree_missing: false,
        merge_error: None,
        last_capture: None,
        last_spinner: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
        mcp_active: false,
        pane_pid: None,
        tracked_command: None,
        subagents: Vec::new(),
        children: Vec::new(),
    }
}

fn repo(path: &str, agents: Vec<AgentNode>) -> RepoNode {
    RepoNode {
        path: path.into(),
        name: path.to_string(),
        remote_url: None,
        pinned: false,
        agents,
        dropr: None,
        dropr_tasks: Vec::new(),
        main_status: None,
        main_last_capture: None,
        main_last_spinner: None,
        main_last_change_at: None,
        main_shell_working: false,
        main_mcp_active: false,
        main_pane_pid: None,
        main_tracked_command: None,
        main_subagents_active: 0,
    }
}

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

fn install_job(app: &mut App, repo_path: &str, agent_id: &str) {
    let (_sender, receiver) = mpsc::channel();
    app.merge_job = Some(MergeJob {
        repo_path: repo_path.into(),
        agent_id: agent_id.into(),
        branch: format!("feature/{agent_id}"),
        step: MERGING_PR,
        receiver,
    });
}

#[test]
fn progress_labels_are_stable_and_specific() {
    assert_eq!(
        [MERGING_PR, PULLING_MAIN, CLEANING_UP],
        ["merging PR", "pulling main", "cleaning up",]
    );
}

#[test]
fn navigation_remains_available_during_merge() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    install_job(&mut app, "/repo", "wanted");

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected, 1);
    assert!(app.merge_job.is_some());
}

#[test]
fn normal_quit_requires_force_quit_during_merge() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    install_job(&mut app, "/repo", "wanted");

    for code in [KeyCode::Char('q'), KeyCode::Esc] {
        assert!(
            !app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
                .unwrap()
        );
        assert!(app.message.as_ref().is_some_and(|(message, _)| {
            message.contains("merge in progress: feature/wanted")
                && message.contains("ctrl-c to force quit")
        }));
    }

    assert!(
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL,))
            .unwrap()
    );
}

#[test]
fn ctrl_c_force_quits_from_prompt_while_merging() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    install_job(&mut app, "/repo", "wanted");
    app.mode = Mode::PromptAgent {
        repo: 0,
        with_prompt: false,
        input: String::new(),
    };

    assert!(
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .unwrap()
    );
    let Mode::PromptAgent { input, .. } = &app.mode else {
        panic!("prompt mode changed");
    };
    assert!(input.is_empty());
}

#[test]
fn merge_progress_banner_is_scoped_to_matching_agent() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    install_job(&mut app, "/repo", "wanted");
    let notice =
        crate::ui::merge_dialog::notice_lines(&app, Some(Selection::Agent { repo: 0, agent: 0 }));
    assert!(notice[0].to_string().starts_with("MERGING feature/wanted"));
    assert!(notice[0].to_string().contains(MERGING_PR));
    assert!(crate::ui::merge_dialog::notice_lines(&app, Some(Selection::Repo(0))).is_empty());
    assert!(crate::ui::merge_dialog::notice_lines(&app, None).is_empty());
    assert_eq!(
        crate::ui::merge_dialog::preview_title(&app, Some(Selection::Agent { repo: 0, agent: 0 }),)
            .unwrap()
            .to_string()
            .trim(),
        "MERGING feature/wanted · merging PR"
    );
    assert!(crate::ui::merge_dialog::preview_title(&app, Some(Selection::Repo(0))).is_none());
}

#[test]
fn merge_outcome_notice_is_scoped_to_matching_agent() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted"), agent("not-wanted")])];
    app.merge_outcome = Some(MergeOutcome {
        repo_path: "/repo".into(),
        agent_id: "wanted".into(),
        branch: "feature/wanted".into(),
        result: Err("failed detail".into()),
    });
    let notice =
        crate::ui::merge_dialog::notice_lines(&app, Some(Selection::Agent { repo: 0, agent: 0 }));
    assert!(notice.iter().any(|line| line.to_string() == "MERGE FAILED"));
    assert!(
        notice
            .iter()
            .any(|line| line.to_string() == "failed detail")
    );
    assert!(crate::ui::merge_dialog::notice_lines(&app, Some(Selection::Repo(0))).is_empty());
    assert!(
        crate::ui::merge_dialog::notice_lines(&app, Some(Selection::Agent { repo: 0, agent: 1 }),)
            .is_empty()
    );
    assert_eq!(
        crate::ui::merge_dialog::preview_title(&app, Some(Selection::Agent { repo: 0, agent: 0 }),)
            .unwrap()
            .to_string()
            .trim(),
        "MERGE FAILED feature/wanted"
    );
}

#[test]
fn merge_outcome_can_be_dismissed() {
    let mut app = test_app();
    let outcome = MergeOutcome {
        repo_path: "/repo".into(),
        agent_id: "wanted".into(),
        branch: "feature/wanted".into(),
        result: Err("failed detail".into()),
    };
    app.merge_outcome = Some(outcome.clone());

    assert!(app.dismiss_merge_outcome());
    assert!(app.merge_outcome().is_none());
    assert!(!app.dismiss_merge_outcome());

    app.merge_outcome = Some(outcome);
    install_job(&mut app, "/repo", "wanted");
    assert!(!app.dismiss_merge_outcome());
    assert!(app.merge_outcome().is_some());
}

#[test]
fn escape_dismisses_merge_outcome_before_quitting() {
    let mut app = test_app();
    app.merge_outcome = Some(MergeOutcome {
        repo_path: "/repo".into(),
        agent_id: "wanted".into(),
        branch: "feature/wanted".into(),
        result: Ok(()),
    });

    assert!(
        !app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap()
    );
    assert!(app.merge_outcome().is_none());
    assert!(
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap()
    );
}

#[test]
fn second_merge_is_rejected_without_replacing_worker() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    install_job(&mut app, "/repo", "wanted");

    app.start_merge(0, 0);

    assert!(app.merge_job.is_some());
    assert_eq!(app.merge_job().unwrap().agent_id, "wanted");
    assert!(
        app.message
            .as_ref()
            .is_some_and(|(message, _)| message.contains("already in progress"))
    );
}

#[test]
fn merge_command_is_rejected_while_worker_is_running() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    app.selected = 1;
    install_job(&mut app, "/repo", "wanted");

    app.merge_selected();

    assert!(matches!(app.mode, Mode::Normal));
    assert!(
        app.message
            .as_ref()
            .is_some_and(|(message, _)| message.contains("already in progress"))
    );
}

#[test]
fn destructive_actions_are_rejected_for_merging_agent() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    install_job(&mut app, "/repo", "wanted");

    app.kill_agent(0, 0).unwrap();
    assert_eq!(app.registry.repos[0].agents.len(), 1);
    assert!(
        app.message
            .as_ref()
            .is_some_and(|(message, _)| message.contains("while it is merging"))
    );

    app.delete_agent_branch(0, 0).unwrap();
    assert_eq!(app.registry.repos[0].agents.len(), 1);
    assert!(
        app.message
            .as_ref()
            .is_some_and(|(message, _)| message.contains("while its agent is merging"))
    );
}

#[test]
fn restart_is_rejected_for_merging_agent() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    app.selected = app
        .visible()
        .iter()
        .position(|selection| matches!(selection, Selection::Agent { .. }))
        .unwrap();
    install_job(&mut app, "/repo", "wanted");

    app.restart_selected().unwrap();

    assert!(app.merge_job.is_some());
    assert!(
        app.message.as_ref().is_some_and(|(message, _)| {
            message == "cannot restart an agent while it is merging"
        })
    );
}

#[test]
fn error_dialog_force_kill_is_rejected_for_merging_agent() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    install_job(&mut app, "/repo", "wanted");
    app.mode = Mode::ErrorDialog {
        title: "kill failed".into(),
        lines: vec!["failed".into()],
        force_kill: Some(crate::ui::ForceKillTarget {
            repo_path: "/repo".into(),
            agent_id: "wanted".into(),
        }),
    };

    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.registry.repos[0].agents.len(), 1);
    assert!(
        app.message
            .as_ref()
            .is_some_and(|(message, _)| message.contains("while it is merging"))
    );
}

#[test]
fn failure_resolves_agent_identity_after_index_drift() {
    let mut app = test_app();
    app.registry.repos = vec![
        repo("/repo-a", vec![agent("other")]),
        repo("/repo-b", vec![agent("first"), agent("wanted")]),
    ];
    app.registry.repos.swap(0, 1);
    app.registry.repos[0].agents.swap(0, 1);
    install_job(&mut app, "/repo-b", "wanted");
    app.merge_job.as_mut().unwrap().step = CLEANING_UP;

    app.finish_merge(Err("merge failed".into())).unwrap();

    assert!(app.merge_job.is_none());
    assert_eq!(app.registry.repos[0].agents[0].id, "wanted");
    assert_eq!(
        app.registry.repos[0].agents[0].merge_error.as_deref(),
        Some("merge failed")
    );
    assert!(app.registry.repos[0].agents[1].merge_error.is_none());
}

#[test]
fn success_resolves_identity_after_selection_and_index_drift() {
    let mut app = test_app();
    app.registry.repos = vec![
        repo("/repo-a", vec![agent("other")]),
        repo("/repo-b", vec![agent("first"), agent("wanted")]),
    ];
    install_job(&mut app, "/repo-b", "wanted");
    app.registry.repos.swap(0, 1);
    app.registry.repos[0].agents.swap(0, 1);
    app.selected = 3;

    app.finish_merge_with(Ok(()), |_| Ok(())).unwrap();

    assert_eq!(
        app.registry.repos[0]
            .agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>(),
        ["first"]
    );
    assert!(
        app.registry.repos[1]
            .agents
            .iter()
            .any(|agent| agent.id == "other")
    );
    assert!(app.merge_job.is_none());
    assert_eq!(app.merge_outcome().unwrap().result, Ok(()));
    assert!(app.selected < app.visible().len());
}

#[test]
fn success_remaps_dialog_to_same_later_agent() {
    let mut app = test_app();
    app.registry.repos = vec![repo(
        "/repo",
        vec![agent("merged"), agent("middle"), agent("dialog-target")],
    )];
    install_job(&mut app, "/repo", "merged");
    app.mode = Mode::ConfirmKill { repo: 0, agent: 2 };

    app.finish_merge_with(Ok(()), |_| Ok(())).unwrap();

    let Mode::ConfirmKill { repo, agent } = app.mode else {
        panic!("dialog was closed");
    };
    assert_eq!((repo, agent), (0, 1));
    assert_eq!(app.registry.repos[repo].agents[agent].id, "dialog-target");
}

#[test]
fn success_closes_dialog_for_removed_agent_defensively() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("merged"), agent("other")])];
    install_job(&mut app, "/repo", "merged");
    app.mode = Mode::ConfirmDeleteBranch { repo: 0, agent: 0 };

    app.finish_merge_with(Ok(()), |_| Ok(())).unwrap();

    assert!(matches!(app.mode, Mode::Normal));
    assert!(
        app.message.as_ref().is_some_and(|(message, _)| {
            message == "closed dialog because its agent was merged"
        })
    );
}

#[test]
fn disconnected_worker_uses_merge_error_path() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    let (sender, receiver) = mpsc::channel();
    app.merge_job = Some(MergeJob {
        repo_path: "/repo".into(),
        agent_id: "wanted".into(),
        branch: "feature/wanted".into(),
        step: MERGING_PR,
        receiver,
    });
    drop(sender);

    app.drain_merge_events().unwrap();

    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.merge_job.is_none());
    assert_eq!(
        app.registry.repos[0].agents[0].merge_error.as_deref(),
        Some(WORKER_TERMINATED)
    );
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some(WORKER_TERMINATED)
    );
    assert_eq!(
        app.merge_outcome()
            .unwrap()
            .result
            .as_ref()
            .map_err(String::as_str),
        Err(WORKER_TERMINATED)
    );
}
