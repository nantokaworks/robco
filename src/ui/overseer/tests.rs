use super::*;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase};
use crate::{config::Config, model::OverseerCategory, registry::Registry};
use ratatui::style::Color;

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
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
    let mut snapshot = OverseerSnapshot::default();

    // Daemon dead -> Dead regardless of dispatch.
    snapshot.daemon_alive = false;
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
    append_ledger(&mut lines, &OverseerConfig::default(), &ledger);
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
    append_ledger(&mut lines, &OverseerConfig::default(), &Ledger::default());
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
    append_ledger(&mut lines, &OverseerConfig::default(), &ledger);
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("active phases"));
    assert!(rendered.contains("working=1"));
    assert!(!rendered.contains("merged"));
}
