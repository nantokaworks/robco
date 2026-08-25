use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    config::Config,
    model::{AgentNode, Status},
    registry::Registry,
};

fn agent(id: &str) -> AgentNode {
    let now = Local::now();
    AgentNode {
        id: id.to_string(),
        parent_agent_id: None,
        title: id.to_string(),
        task_number: None,
        worktree_path: format!("/worktrees/{id}").into(),
        branch: format!("feature/{id}"),
        base_commit: String::new(),
        program: "codex".to_string(),
        spawned_by_version: None,
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
        dropr_tasks: crate::dropr::DroprTaskFetch::default(),
        main_status: None,
        main_last_capture: None,
        main_last_spinner: None,
        main_last_change_at: None,
        main_shell_working: false,
        main_mcp_active: false,
        main_pane_pid: None,
        main_tracked_command: None,
        main_subagents_active: 0,
        main_behind_origin: None,
        checkout_state: None,
    }
}

#[test]
fn pr_request_guards_invalid_targets() {
    let repos = vec![repo("/repo", vec![agent("one")])];

    assert_eq!(
        pr_target_for_selection(&repos, Some(Selection::Repo(0))),
        Err("select an agent to request a PR")
    );
    assert_eq!(
        pr_target_for_selection(
            &repos,
            Some(Selection::ChildWorktree {
                repo: 0,
                agent: 0,
                child: 0,
            }),
        ),
        Err("PR request is not available for child worktrees")
    );
}

#[test]
fn confirm_pr_enter_forwards_edited_prompt_to_request() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.registry.repos = vec![repo("/repo", vec![agent("one")])];
    app.mode = Mode::ConfirmPr {
        repo_path: "/repo".into(),
        agent_id: "one".into(),
        branch: "feature/one".into(),
        input: "edited prompt".into(),
        approval_head: None,
    };
    let mut sent = None;

    app.handle_key_with_pr_sender(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        |session, prompt| {
            sent = Some((session.to_string(), prompt.to_string()));
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(sent, Some(("robco_one".into(), "edited prompt".into())));
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn confirm_pr_target_resolves_after_reordering_and_not_after_pruning() {
    let target = PrTarget {
        repo_path: "/repo-b".into(),
        agent_id: "wanted".to_string(),
        branch: "feature/wanted".to_string(),
    };
    let mut repos = vec![
        repo("/repo-a", vec![agent("other")]),
        repo("/repo-b", vec![agent("first"), agent("wanted")]),
    ];
    repos.swap(0, 1);
    repos[0].agents.swap(0, 1);

    assert_eq!(
        resolve_agent(&repos, &target.repo_path, &target.agent_id),
        Some((0, 0))
    );

    repos[0].agents.remove(0);
    assert_eq!(
        resolve_agent(&repos, &target.repo_path, &target.agent_id),
        None
    );
}
