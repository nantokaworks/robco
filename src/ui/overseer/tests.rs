use super::*;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase};
use ratatui::style::Color;

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
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("heartbeat");
    fs::write(&path, "tick").unwrap();
    let config = OverseerConfig {
        failure_circuit_threshold: 3,
        ..OverseerConfig::default()
    };

    // Exactly at the threshold the circuit is open — pin the equality boundary
    // so a `>=` -> `>` regression is caught here too.
    let mut open = Ledger::default();
    open.counters.consecutive_failures = 3;
    let mut lines = Vec::new();
    super::render::append_health(&mut lines, &config, &open, &path);
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(rendered.contains("circuit: OPEN"));
    assert!(rendered.contains("robco overseer set dispatch on"));

    let mut closed = Ledger::default();
    closed.counters.consecutive_failures = 2;
    let mut lines = Vec::new();
    super::render::append_health(&mut lines, &config, &closed, &path);
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(!rendered.contains("robco overseer set dispatch on"));
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
