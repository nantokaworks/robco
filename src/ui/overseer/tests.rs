use super::*;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase};
use crate::{
    config::Config,
    model::{AgentNode, ManagementMode, OverseerCategory, RepoNode, Status},
    registry::Registry,
};
use chrono::Local;
use ratatui::style::Color;

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

fn overseer_worker(id: &str, title: &str, management: ManagementMode) -> AgentNode {
    let now = Local::now();
    AgentNode {
        id: id.into(),
        parent_agent_id: Some(crate::overseer::OVERSEER_AGENT_ID.into()),
        management,
        title: title.into(),
        worktree_path: format!("/tmp/{id}").into(),
        branch: id.into(),
        base_commit: String::new(),
        program: "codex".into(),
        profile: None,
        tmux_session: id.into(),
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

fn management_app() -> App {
    let mut app = test_app();
    app.registry.repos.push(RepoNode {
        path: "/tmp/repo".into(),
        name: "repo".into(),
        remote_url: None,
        pinned: false,
        agents: vec![
            overseer_worker("auto-agent", "auto title", ManagementMode::Auto),
            overseer_worker("manual-agent", "manual title", ManagementMode::Manual),
        ],
        dropr: None,
        dropr_tasks: Vec::new(),
        main_status: None,
        main_last_capture: None,
        main_last_change_at: None,
        main_shell_working: false,
        main_pane_pid: None,
        main_tracked_command: None,
        main_subagents_active: 0,
    });
    app.overseer_snapshot.ledger.entries = vec![
        LedgerEntry {
            task_id: "auto-task".into(),
            display_id: "#1".into(),
            repo: "repo".into(),
            agent_id: "auto-agent".into(),
            branch: "auto".into(),
            phase: LedgerPhase::Working,
            dispatched_at: Utc::now(),
            retries: 0,
            pr_url: None,
        },
        LedgerEntry {
            task_id: "manual-task".into(),
            display_id: String::new(),
            repo: "repo".into(),
            agent_id: "manual-agent".into(),
            branch: "manual".into(),
            phase: LedgerPhase::Claimed,
            dispatched_at: Utc::now(),
            retries: 0,
            pr_url: None,
        },
    ];
    app
}

#[test]
fn flags_line_joins_and_reds_warnings() {
    let line = super::render::flags_line(&[
        ("dispatch", "on".into(), false),
        ("circuit", "OPEN".into(), true),
    ]);
    let rendered = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(rendered, "dispatch: on · circuit: OPEN");
    assert_eq!(
        line.spans
            .iter()
            .find(|span| span.content == "OPEN")
            .unwrap()
            .style
            .fg,
        Some(Color::Red)
    );
    assert_ne!(
        line.spans
            .iter()
            .find(|span| span.content == "on")
            .unwrap()
            .style
            .fg,
        Some(Color::Red)
    );
}

#[test]
fn status_stops_animating_when_dispatch_off_but_daemon_alive() {
    // Regression for #172: after the `S` panic-stop the daemon stays alive
    // while dispatch flips off. The OVERSEER row must render a static glyph
    // instead of the `Running` spinner that keeps animating forever.
    // Daemon dead -> Dead regardless of dispatch.
    let mut snapshot = OverseerSnapshot {
        daemon_alive: false,
        ..Default::default()
    };
    snapshot.overseer.dispatch_enabled = true;
    assert_eq!(snapshot.status(), Status::Dead);

    // Daemon alive + dispatch on -> Running (animated spinner).
    snapshot.daemon_alive = true;
    snapshot.overseer.dispatch_enabled = true;
    assert_eq!(snapshot.status(), Status::Running);

    // Daemon alive + dispatch off -> Idle (static, non-animated).
    snapshot.overseer.dispatch_enabled = false;
    assert_eq!(snapshot.status(), Status::Idle);
}

#[test]
fn stale_heartbeat_is_not_fresh() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("heartbeat");
    fs::write(&path, "tick").unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(heartbeat_is_fresh_at(
        &path,
        10,
        modified + Duration::from_secs(20)
    ));
    assert!(!heartbeat_is_fresh_at(
        &path,
        10,
        modified + Duration::from_secs(21)
    ));
    assert!(!heartbeat_is_fresh_at(
        &temp.path().join("missing"),
        10,
        modified
    ));
}

#[test]
fn open_circuit_shows_recovery_hint() {
    let config = OverseerConfig {
        failure_circuit_threshold: 3,
        ..OverseerConfig::default()
    };

    // Exactly at the threshold the circuit is open — pin the equality boundary
    // so a `>=` -> `>` regression is caught here too.
    let mut open = Ledger::default();
    open.counters.consecutive_failures = 3;
    let mut lines = Vec::new();
    super::render::append_health(&mut lines, &config, &open, false, None);
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(rendered.contains("circuit: OPEN"));
    assert!(rendered.contains("robco overseer set dispatch on"));
    assert_eq!(
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content == "OPEN")
            .unwrap()
            .style
            .fg,
        Some(Color::Red)
    );

    let mut closed = Ledger::default();
    closed.counters.consecutive_failures = 2;
    let mut lines = Vec::new();
    super::render::append_health(&mut lines, &config, &closed, false, None);
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(!rendered.contains("robco overseer set dispatch on"));
}

#[test]
fn health_summary_keeps_all_critical_badges() {
    let config = OverseerConfig {
        dispatch_enabled: true,
        failure_circuit_threshold: 2,
        ..OverseerConfig::default()
    };
    let mut ledger = Ledger::default();
    ledger.counters.consecutive_failures = 2;
    let (summary, warn) = super::categories::health_summary_from(&config, &ledger, false);

    assert!(warn);
    assert!(summary.contains("STALE/OFFLINE"));
    assert!(summary.contains("circuit OPEN"));
    assert!(summary.contains("dispatch/no daemon"));
}

#[test]
fn info_pane_reads_dispatch_from_snapshot_not_stale_config() {
    // Regression for #171: the Info pane must render overseer flags from the
    // disk-backed snapshot, not the in-memory `app.config` that only the `,`
    // settings editor refreshes. Simulate an `S` panic-stop landing on disk
    // (snapshot reloaded → dispatch off) while `app.config` is still stale "on".
    let mut app = test_app();
    app.config.overseer.dispatch_enabled = true;
    app.overseer_snapshot.overseer.dispatch_enabled = false;

    let (_title, text) = super::summary(&app);
    let rendered = text
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("dispatch: off"));
    assert!(!rendered.contains("dispatch: on"));
}

#[test]
fn info_summary_shows_active_worker_management_counts() {
    let app = management_app();
    let (_, text) = summary(&app);
    let rendered = text
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("management: auto=1, manual=1"));
}

#[test]
fn info_summary_shows_zero_manual_worker_count() {
    let mut app = management_app();
    app.overseer_snapshot
        .ledger
        .entries
        .retain(|entry| entry.agent_id == "auto-agent");
    let (_, text) = summary(&app);
    let rendered = text
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("management: auto=1, manual=0"));
}

#[test]
fn ledger_detail_shows_each_active_worker_management_mode() {
    let app = management_app();
    let lines = category_detail(&app, OverseerCategory::Ledger);
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("worker #1: Auto"));
    assert!(rendered.contains("worker manual title: Manual"));
}

#[test]
fn duplicate_active_agent_is_counted_and_listed_once() {
    let mut app = management_app();
    let mut duplicate = app.overseer_snapshot.ledger.entries[0].clone();
    duplicate.task_id = "duplicate-task".into();
    duplicate.display_id = "#duplicate".into();
    app.overseer_snapshot.ledger.entries.push(duplicate);

    let (_, text) = summary(&app);
    let summary_rendered = text
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(summary_rendered.contains("management: auto=1, manual=1"));

    let lines = category_detail(&app, OverseerCategory::Ledger);
    let detail_rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert_eq!(detail_rendered.matches("worker #1: Auto").count(), 1);
    assert!(!detail_rendered.contains("worker #duplicate"));
}

#[test]
fn every_category_has_summary_detail_and_preview() {
    let app = test_app();
    for category in OverseerCategory::ALL {
        let (summary, _) = category_summary(&app, category);
        let detail = category_detail(&app, category);
        let (title, preview) = category_preview(&app, category);

        assert!(!summary.is_empty());
        assert!(!detail.is_empty());
        assert_eq!(title, format!("OVERSEER / {}", category.label()));
        assert_eq!(preview.lines, detail);
    }
}

#[test]
fn stale_dispatch_counter_renders_zero() {
    let today = Utc::now().date_naive();
    let mut ledger = Ledger::default();
    ledger.counters.date = today.pred_opt();
    ledger.counters.dispatched_today = 7;
    let mut lines = Vec::new();
    append_ledger(&mut lines, &OverseerConfig::default(), &ledger, &[]);
    let rendered = lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(rendered.starts_with("dispatches: 0 / "));
}

#[test]
fn empty_ledger_hides_empty_detail_lines() {
    let mut lines = Vec::new();
    append_ledger(
        &mut lines,
        &OverseerConfig::default(),
        &Ledger::default(),
        &[],
    );
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(!rendered.contains("workers by repo"));
    assert!(!rendered.contains("active phases"));
    assert!(!rendered.contains("skip list"));
}

#[test]
fn active_phases_excludes_terminal_entries() {
    let ledger = Ledger {
        entries: vec![
            LedgerEntry {
                task_id: "active".into(),
                display_id: "#1".into(),
                repo: "repo".into(),
                agent_id: "agent".into(),
                branch: "active".into(),
                phase: LedgerPhase::Working,
                dispatched_at: Utc::now(),
                retries: 0,
                pr_url: None,
            },
            LedgerEntry {
                task_id: "terminal".into(),
                display_id: "#2".into(),
                repo: "repo".into(),
                agent_id: "agent".into(),
                branch: "terminal".into(),
                phase: LedgerPhase::Merged,
                dispatched_at: Utc::now(),
                retries: 0,
                pr_url: None,
            },
        ],
        ..Ledger::default()
    };
    let mut lines = Vec::new();
    append_ledger(&mut lines, &OverseerConfig::default(), &ledger, &[]);
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("active phases"));
    assert!(rendered.contains("working=1"));
    assert!(!rendered.contains("merged"));
}
