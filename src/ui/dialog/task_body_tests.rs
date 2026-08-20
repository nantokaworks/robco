use ratatui::{Terminal, backend::TestBackend, style::Modifier};

use crate::{
    config::Config,
    dropr::{DroprTaskCandidate, DroprTaskFetch},
    registry::Registry,
};

use super::super::{App, Mode, draw};

fn task(display_id: &str, description: Option<&str>) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_string(),
        title: format!("Task {display_id}"),
        description: description.map(str::to_string),
        priority: String::new(),
        status: "open".to_string(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
        id: format!("id-{display_id}"),
        parent_task_id: None,
        child_count: 0,
    }
}

fn app_with_task(description: Option<&str>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.registry.repos = vec![crate::model::RepoNode {
        path: "/repo".into(),
        name: "repo".into(),
        remote_url: None,
        pinned: true,
        agents: Vec::new(),
        dropr: None,
        dropr_tasks: DroprTaskFetch {
            tasks: vec![task("#1", description)],
            problems: Vec::new(),
            answered: true,
            subtrees_known: Default::default(),
        },
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
    }];
    // The fixture's repo path is not under the app's launch dir, so it
    // renders under "other locations" rather than at index 0.
    app.selected = app
        .visible()
        .iter()
        .position(|selection| matches!(selection, crate::model::Selection::Repo(0)))
        .expect("repo row is visible");
    app
}

fn draw_task_body(app: &App) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, app);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn buffer_contains(buffer: &ratatui::buffer::Buffer, needle: &str) -> bool {
    let mut rows = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            rows.push_str(buffer.cell((x, y)).unwrap().symbol());
        }
        rows.push('\n');
    }
    rows.contains(needle)
}

#[test]
fn renders_the_focused_tasks_title_and_body_over_a_dimmed_backdrop() {
    let mut app = app_with_task(Some("the task body text"));
    app.mode = Mode::TaskBody { task: 0, scroll: 0 };

    let buffer = draw_task_body(&app);

    assert!(buffer_contains(&buffer, "repo / #1"));
    assert!(buffer_contains(&buffer, "the task body text"));
    // `layout::root` insets a 1-row/1-col CRT-bezel margin, so (1, 1) is the
    // body area's own top-left corner — outside any popup this small
    // terminal could center, so it must still carry the backdrop's dim
    // modifier.
    assert!(
        buffer
            .cell((1, 1))
            .unwrap()
            .modifier
            .contains(Modifier::DIM)
    );
}

#[test]
fn no_repo_selected_draws_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.mode = Mode::TaskBody { task: 0, scroll: 0 };

    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    let mut cursor = None;
    terminal.draw(|frame| cursor = draw(frame, &app)).unwrap();

    assert_eq!(cursor, None);
}

#[test]
fn a_stale_task_index_draws_nothing() {
    let mut app = app_with_task(None);
    app.mode = Mode::TaskBody { task: 5, scroll: 0 };

    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    let mut cursor = None;
    terminal.draw(|frame| cursor = draw(frame, &app)).unwrap();

    assert_eq!(cursor, None);
}

#[test]
fn scroll_clamps_to_what_the_content_actually_has() {
    let long_description = (1..=40)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = app_with_task(Some(&long_description));
    app.mode = Mode::TaskBody {
        task: 0,
        scroll: u16::MAX,
    };
    let maxed_out = draw_task_body(&app);

    app.mode = Mode::TaskBody {
        task: 0,
        scroll: 1000,
    };
    let clamped_to_1000 = draw_task_body(&app);

    // Both scroll values sit far past the content's actual length, so both
    // clamp to the same maximum and must render identically — a Paragraph
    // that clamped neither would show blank space past the content instead.
    assert_eq!(maxed_out.content(), clamped_to_1000.content());
    assert!(buffer_contains(&maxed_out, "line 40"));
}
