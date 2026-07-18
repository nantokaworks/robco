use chrono::Local;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

use super::*;
use crate::{
    config::Config,
    model::{AgentNode, RepoNode, Status},
    registry::Registry,
    ui::actions::merge::MergeOutcome,
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
        profile: None,
        tmux_session: format!("robco_{id}"),
        created_at: now,
        updated_at: now,
        status: Status::Running,
        worktree_missing: false,
        merge_error: None,
        last_capture: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
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
        main_last_change_at: None,
        main_shell_working: false,
        main_pane_pid: None,
        main_tracked_command: None,
        main_subagents_active: 0,
    }
}

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    app.merge_outcome = Some(MergeOutcome {
        repo_path: "/repo".into(),
        agent_id: "wanted".into(),
        branch: "feature/wanted".into(),
        result: Err("boom".into()),
    });
    app
}

fn rendered(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer.cell((x, y)).unwrap().symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn matching_agent_renders_merge_failure_overlay() {
    let app = test_app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal
        .draw(|frame| draw(frame, &app, Some(Selection::Agent { repo: 0, agent: 0 })))
        .unwrap();

    assert!(rendered(&terminal).contains("MERGE FAILED"));
}

#[test]
fn non_matching_selection_does_not_render_merge_failure_overlay() {
    let app = test_app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal
        .draw(|frame| draw(frame, &app, Some(Selection::Repo(0))))
        .unwrap();

    assert!(!rendered(&terminal).contains("MERGE FAILED"));
}

#[test]
fn merge_notice_clamps_to_tiny_area() {
    let app = test_app();
    let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
    let area = Rect::new(0, 0, 8, 2);

    terminal
        .draw(|frame| {
            render_merge_notice(
                frame,
                &app,
                Some(Selection::Agent { repo: 0, agent: 0 }),
                area,
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert!(
        (area.height..buffer.area.height).all(|y| {
            (0..buffer.area.width).all(|x| buffer.cell((x, y)).unwrap().symbol() == " ")
        })
    );
}
