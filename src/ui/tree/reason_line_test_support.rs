//! Fixtures shared by the reason line's test modules — the `merge_error`
//! coverage in `reason_line_tests.rs`, the terminal-ledger-phase coverage in
//! `reason_line_phase_tests.rs`, and the live-hold coverage in
//! `reason_line_hold_tests.rs`. Split out so no test file has to carry the
//! whole harness and cross the source-size limit.

use chrono::Utc;

use crate::config::Config;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase, MergeHold};
use crate::overseer::logging::{DecisionEntry, DecisionKind};
use crate::registry::Registry;
use crate::ui::App;
use crate::ui::test_support;
use crate::ui::tree::render_test_support::rendered_rows_at_width;

/// Wide enough for the tree pane to reach its own maximum width, so these
/// tests read the reason line's layout rather than the truncation every row
/// already gets at the 24-column minimum.
pub(super) const WIDE: u16 = 160;

/// The narrowest tree pane the layout allows, for the clipping case.
pub(super) const NARROW: u16 = 60;

/// Flat agents in one local repo, titles kept short.
pub(super) fn app_with(names: &[&str]) -> App {
    app_from(names, |_| None)
}

/// The same fixture, with `parent` deciding each agent's parent id by
/// position — enough for the nested cases the guide columns must survive.
pub(super) fn app_from(names: &[&str], parent: impl Fn(usize) -> Option<String>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::default();
    let agents = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            // Only worktrees under `worktree_root` survive
            // `prune_unmanaged_agents`.
            let mut agent = test_support::agent(name, config.worktree_root.join(name));
            agent.parent_agent_id = parent(index);
            agent
        })
        .collect();
    let registry = Registry {
        version: 1,
        repos: vec![test_support::repo(temp.path().join("repo"), agents)],
    };
    let mut app = App::new(registry, config, temp.path().into());
    // Ignore host tmux sessions and the OVERSEER pane so the rows are
    // deterministic and start at the top of the frame.
    app.orphans = Vec::new();
    app.overseer_visible = false;
    app
}

pub(super) fn rows(app: &App) -> Vec<String> {
    rendered_rows_at_width(app, WIDE)
}

pub(super) fn set_error(app: &mut App, name: &str, error: Option<&str>) {
    let agent = app.registry.repos[0]
        .agents
        .iter_mut()
        .find(|agent| agent.title == name)
        .expect("agent in fixture");
    agent.merge_error = error.map(str::to_string);
}

/// The rendered reason line for a stopped agent (`merge_error` or a terminal
/// ledger phase), trailing blanks trimmed.
pub(super) fn reason_row(rows: &[String]) -> Option<String> {
    rows.iter()
        .find(|row| row.contains('⚠'))
        .map(|row| row.trim_end().to_string())
}

/// The rendered reason line for a live merge hold, trailing blanks trimmed.
/// Its own glyph (`⏸`), never `⚠`, is what tells the two apart on screen.
pub(super) fn held_row(rows: &[String]) -> Option<String> {
    rows.iter()
        .find(|row| row.contains('⏸'))
        .map(|row| row.trim_end().to_string())
}

pub(super) fn drawn_rows(rows: &[String]) -> usize {
    rows.iter().filter(|row| !row.trim().is_empty()).count()
}

/// Park `name`'s ledger entry in `phase`, with the task id the decisions below
/// are recorded against. The agent's own id is its fixture name.
pub(super) fn set_phase(app: &mut App, name: &str, task_id: &str, phase: LedgerPhase) {
    app.overseer_snapshot
        .ledger
        .entries
        .push(ledger_entry(name, task_id, phase));
}

/// Park `name`'s ledger entry `PrOpened` and held on `reason`, charged for
/// `passes` consecutive polls — the same field `merge_hold::charge` writes.
pub(super) fn set_hold(app: &mut App, name: &str, task_id: &str, reason: &str, passes: u32) {
    let mut entry = ledger_entry(name, task_id, LedgerPhase::PrOpened);
    entry.merge_hold = MergeHold {
        reason: Some(reason.into()),
        head: Some("f002d389".into()),
        passes,
        escalated: false,
    };
    app.overseer_snapshot.ledger.entries.push(entry);
}

/// Lifts whatever hold `set_hold` recorded for `name`, the way the ledger
/// itself clears it once the next auto-merge pass gets past the gate.
pub(super) fn clear_hold(app: &mut App, name: &str) {
    let entry = app
        .overseer_snapshot
        .ledger
        .entries
        .iter_mut()
        .find(|entry| entry.agent_id == name)
        .expect("held entry in fixture");
    entry.merge_hold = MergeHold::default();
}

/// Append one decision for `task_id`, in the order a real log would hold it —
/// oldest first, which is what `terminal_reason`'s newest-wins read depends on.
pub(super) fn log_decision(app: &mut App, kind: DecisionKind, task_id: &str, reason: &str) {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(task_id.into());
    app.overseer_snapshot.decisions.push(entry);
}

/// The same as [`log_decision`], but stamped with the source
/// `daemon::discord_events` uses for its phase mirror.
pub(super) fn log_phase_mirror(app: &mut App, kind: DecisionKind, task_id: &str, reason: &str) {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(task_id.into());
    entry.source = Some("daemon_event".into());
    app.overseer_snapshot.decisions.push(entry);
}

fn ledger_entry(agent_id: &str, task_id: &str, phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: task_id.into(),
        display_id: task_id.into(),
        repo: "nantokaworks/robco".into(),
        agent_id: agent_id.into(),
        branch: format!("robco/{agent_id}"),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        escalation_notified_reason: None,
        escalation_notified_head: None,
        worker_escalated: false,
        operator_override: None,
        merge_approval: None,
        pr_facts: None,
        worker_finished_at: None,
        approval_dropped: None,
    }
}
