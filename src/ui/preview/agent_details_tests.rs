use chrono::Utc;

use super::*;
use crate::{
    config::Config,
    model::Status,
    overseer::{
        ledger::{LedgerPhase, new_entry},
        logging::{DecisionEntry, DecisionKind},
    },
    registry::Registry,
    ui::test_support,
};

fn stopped_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut agent = test_support::agent("worker", temp.path().join("worker"));
    agent.status = Status::Idle;
    let repo = test_support::repo(temp.path().join("repo"), vec![agent]);
    let config = Config {
        worktree_root: temp.path().into(),
        ..Config::default()
    };
    let mut app = App::new(
        Registry {
            version: 1,
            repos: vec![repo],
        },
        config,
        temp.path().into(),
    );
    let agent = &app.registry.repos[0].agents[0];
    let mut entry = new_entry(agent, "repo", Utc::now());
    entry.phase = LedgerPhase::Failed;
    app.overseer_snapshot.ledger.entries.push(entry);
    app
}

#[test]
fn info_contains_every_terminal_reason_line() {
    let mut app = stopped_app();
    let mut decision = DecisionEntry::new(DecisionKind::Hold, "first line\nsecond line");
    decision.task = Some("worker".into());
    app.overseer_snapshot.decisions.push(decision);

    let rendered = lines(&app, &app.registry.repos[0].agents[0])
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    assert!(rendered.iter().any(|line| line == "stopped: first line"));
    assert!(rendered.iter().any(|line| line == "         second line"));
}

#[test]
fn info_includes_held_text_not_covered_by_merge_hold_detail() {
    let mut app = stopped_app();
    let entry = &mut app.overseer_snapshot.ledger.entries[0];
    entry.phase = LedgerPhase::PrOpened;
    entry.approval_dropped = Some("approval dropped after worker push".into());

    let rendered = lines(&app, &app.registry.repos[0].agents[0])
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    assert!(
        rendered
            .iter()
            .any(|line| line == "held: approval dropped after worker push")
    );
}
