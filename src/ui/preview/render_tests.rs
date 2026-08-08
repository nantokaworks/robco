use chrono::Local;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

use super::*;
use crate::{
    config::Config,
    model::{AgentNode, RepoNode, Status},
    registry::Registry,
    ui::{actions::merge::MergeOutcome, panes_for},
};

fn agent(id: &str) -> AgentNode {
    let now = Local::now();
    AgentNode {
        management: crate::model::ManagementMode::Manual,
        id: id.to_string(),
        parent_agent_id: None,
        title: id.to_string(),
        task_number: None,
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
        management: crate::model::ManagementMode::Auto,
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

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    app.merge_outcomes.insert(
        "/repo".into(),
        MergeOutcome {
            repo_path: "/repo".into(),
            agent_id: "wanted".into(),
            branch: "feature/wanted".into(),
            result: Err("boom".into()),
        },
    );
    app
}

/// The preview block's top border row — the row the tab bar is drawn into, and
/// the one an overlay must never claim.
fn tab_bar_row(app: &App, terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let root = layout::root(buffer.area);
    let preview = layout::panes(root.body, app.overseer_frame_height()).preview;
    (preview.x..preview.x + preview.width)
        .map(|x| buffer.cell((x, preview.y)).unwrap().symbol())
        .collect()
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

const AGENT: Selection = Selection::Agent { repo: 0, agent: 0 };

#[test]
fn matching_agent_renders_merge_failure_in_the_error_tab() {
    let mut app = test_app();
    app.preview = PreviewPane::Error;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal
        .draw(|frame| draw(frame, &app, Some(AGENT)))
        .unwrap();

    let rendered = rendered(&terminal);
    assert!(rendered.contains("MERGE FAILED"));
    assert!(rendered.contains("boom"));
    assert!(rendered.contains("esc dismiss"));
}

#[test]
fn unacknowledged_failure_adds_a_red_tab_without_hiding_the_tab_bar() {
    let app = test_app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal
        .draw(|frame| draw(frame, &app, Some(AGENT)))
        .unwrap();

    // A failure advertises itself in the tab bar but does not take the pane away
    // from whatever the operator was reading.
    assert_ne!(app.preview, PreviewPane::Error);
    let tab_bar = tab_bar_row(&app, &terminal);
    assert!(tab_bar.contains("INFO"), "tab bar was {tab_bar:?}");
    assert!(tab_bar.contains("DIFF"), "tab bar was {tab_bar:?}");
    assert!(tab_bar.contains("TERM"), "tab bar was {tab_bar:?}");

    // Cell colours, not just the label: the tab has to read as a failure from
    // the tab bar alone, without the operator opening it.
    let columns: Vec<char> = tab_bar.chars().collect();
    let err_at = columns
        .windows(3)
        .position(|window| window == ['E', 'R', 'R'])
        .unwrap_or_else(|| panic!("tab bar was {tab_bar:?}"));
    let buffer = terminal.backend().buffer();
    let preview =
        layout::panes(layout::root(buffer.area).body, app.overseer_frame_height()).preview;
    for offset in 0..3 {
        let cell = buffer
            .cell((preview.x + (err_at + offset) as u16, preview.y))
            .expect("the ERR label is inside the preview");
        assert_eq!(cell.fg, ratatui::style::Color::Red, "column {offset}");
    }
}

#[test]
fn dismissed_failure_drops_the_error_tab_and_falls_back_to_a_valid_pane() {
    let mut app = test_app();
    app.selected = app
        .visible()
        .iter()
        .position(|item| *item == AGENT)
        .expect("the agent row is visible");
    app.preview = PreviewPane::Error;
    assert!(app.preview_panes(Some(AGENT)).contains(&PreviewPane::Error));

    assert!(app.dismiss_merge_outcome());

    assert!(!app.preview_panes(Some(AGENT)).contains(&PreviewPane::Error));
    assert_ne!(app.preview, PreviewPane::Error);
    assert!(panes_for(Some(AGENT)).contains(&app.preview));

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| draw(frame, &app, Some(AGENT)))
        .unwrap();
    let rendered = rendered(&terminal);
    assert!(!rendered.contains("ERR"));
    assert!(!rendered.contains("MERGE FAILED"));
}

#[test]
fn non_matching_selection_gets_no_error_tab() {
    let app = test_app();
    assert!(
        !app.preview_panes(Some(Selection::Repo(0)))
            .contains(&PreviewPane::Error)
    );

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| draw(frame, &app, Some(Selection::Repo(0))))
        .unwrap();

    assert!(!rendered(&terminal).contains("MERGE FAILED"));
}

#[test]
fn merge_notice_overlay_leaves_the_tab_bar_readable() {
    let mut app = test_app();
    app.merge_outcomes.insert(
        "/repo".into(),
        MergeOutcome {
            repo_path: "/repo".into(),
            agent_id: "wanted".into(),
            branch: "feature/wanted".into(),
            result: Ok(()),
        },
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal
        .draw(|frame| draw(frame, &app, Some(AGENT)))
        .unwrap();

    assert!(rendered(&terminal).contains("MERGE COMPLETE"));
    let tab_bar = tab_bar_row(&app, &terminal);
    assert!(tab_bar.contains("INFO"), "tab bar was {tab_bar:?}");
    assert!(tab_bar.contains("DIFF"), "tab bar was {tab_bar:?}");
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
