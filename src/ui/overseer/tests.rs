use super::render::append_ledger;
use super::*;
use crate::locale::Locale;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase};
use crate::{
    config::Config,
    model::{OverseerCategory, Status},
    registry::Registry,
};
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
fn status_tracks_daemon_liveness_only() {
    // The daemon always has work to do now — merge polling, Discord/MCP
    // commands, worker monitoring — so status is a plain alive/dead read.
    let mut snapshot = OverseerSnapshot {
        daemon_alive: false,
        ..Default::default()
    };
    assert_eq!(snapshot.status(), Status::Dead);

    snapshot.daemon_alive = true;
    assert_eq!(snapshot.status(), Status::Running);
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
fn health_summary_keeps_the_offline_badge() {
    let (summary, warn) = super::categories::health_summary_from(false, false);

    assert!(warn);
    assert!(summary.contains("STALE/OFFLINE"));
}

#[test]
fn health_frame_shows_merge_recovery_and_flags_it_when_it_cannot_fire() {
    let rendered = |config: &OverseerConfig| {
        let mut lines = Vec::new();
        super::render::append_health(&mut lines, config, true, None, None, None, Locale::En);
        lines
    };
    let text = |lines: &[ratatui::text::Line<'static>]| {
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };

    let off = rendered(&OverseerConfig::default());
    assert!(text(&off).contains("merge-recovery: off"));

    let armed = OverseerConfig {
        auto_merge: true,
        merge_recovery_enabled: true,
        max_merge_recoveries: 2,
        ..OverseerConfig::default()
    };
    assert!(text(&rendered(&armed)).contains("merge-recovery: on (max 2)"));

    // Recovery without auto-merge never fires: no merge is attempted, so no
    // failure is ever handed back. That must read as a warning, not as armed.
    let inert = OverseerConfig {
        auto_merge: false,
        ..armed
    };
    assert_eq!(
        rendered(&inert)
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content == "on (max 2)")
            .unwrap()
            .style
            .fg,
        Some(Color::Red)
    );
}

#[test]
fn health_frame_reports_the_build_the_daemon_started_from() {
    let mut lines = Vec::new();
    super::render::append_health(
        &mut lines,
        &OverseerConfig::default(),
        true,
        None,
        Some("0.1.66"),
        None,
        Locale::En,
    );
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("version: 0.1.66"), "{rendered}");
    assert_ne!(
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content == "0.1.66")
            .unwrap()
            .style
            .fg,
        Some(Color::Red),
        "a daemon on the querying build must not read as an error"
    );
}

#[test]
fn health_frame_flags_a_daemon_running_another_build() {
    let mut lines = Vec::new();
    super::render::append_health(
        &mut lines,
        &OverseerConfig::default(),
        true,
        None,
        Some("0.1.66"),
        Some("daemon is running 0.1.66 but this binary is 0.1.67"),
        Locale::En,
    );
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(
        rendered.contains("daemon is running 0.1.66 but this binary is 0.1.67"),
        "{rendered}"
    );
    assert_eq!(
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content == "0.1.66")
            .unwrap()
            .style
            .fg,
        Some(Color::Red)
    );
}

#[test]
fn a_heartbeat_without_a_build_still_renders_the_health_frame() {
    // Every reader of a heartbeat written before the daemon recorded its build
    // sees `None` here; the frame must stay readable rather than hide the row.
    let mut lines = Vec::new();
    super::render::append_health(
        &mut lines,
        &OverseerConfig::default(),
        true,
        None,
        None,
        None,
        Locale::En,
    );
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("version: unknown"), "{rendered}");
}

#[test]
fn a_stale_build_is_badged_in_the_health_summary() {
    let (summary, warn) = super::categories::health_summary_from(true, true);

    assert!(warn);
    assert!(
        summary.contains(crate::overseer::heartbeat::DRIFT_LABEL),
        "{summary}"
    );
}

#[test]
fn a_dead_daemon_does_not_drift() {
    // The drift warning names a restart. A daemon that is already reported down
    // is getting one regardless, so saying it twice adds no state to act on.
    let mut app = test_app();
    app.overseer_snapshot.daemon_alive = false;
    app.overseer_snapshot.daemon_version = Some("0.0.1".into());

    assert_eq!(app.overseer_snapshot.version_drift(), None);

    app.overseer_snapshot.daemon_alive = true;
    assert!(app.overseer_snapshot.version_drift().is_some());
}

fn category_text(app: &App, category: OverseerCategory) -> String {
    category_detail(app, category)
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

#[test]
fn info_pane_reads_auto_merge_from_snapshot_not_stale_config() {
    // Regression for #171: the Info pane must render overseer flags from the
    // disk-backed snapshot, not the in-memory `app.config` that only the `,`
    // settings editor refreshes. Simulate an external edit landing on disk
    // (snapshot reloaded → auto-merge on) while `app.config` is still stale "off".
    let mut app = test_app();
    app.config.overseer.auto_merge = false;
    app.overseer_snapshot.overseer.auto_merge = true;

    let rendered = category_text(&app, OverseerCategory::Health);

    assert!(rendered.contains("auto-merge: on"));
    assert!(!rendered.contains("auto-merge: off"));
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
fn empty_ledger_hides_empty_detail_lines() {
    let mut lines = Vec::new();
    append_ledger(&mut lines, &Ledger::default(), &[], &Registry::default());
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
                dropr_task_id: None,
                display_id: "#1".into(),
                repo: "repo".into(),
                agent_id: "agent".into(),
                branch: "active".into(),
                phase: LedgerPhase::Working,
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
            },
            LedgerEntry {
                task_id: "terminal".into(),
                dropr_task_id: None,
                display_id: "#2".into(),
                repo: "repo".into(),
                agent_id: "agent".into(),
                branch: "terminal".into(),
                phase: LedgerPhase::Merged,
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
            },
        ],
        ..Ledger::default()
    };
    let mut lines = Vec::new();
    append_ledger(&mut lines, &ledger, &[], &Registry::default());
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("active phases"));
    assert!(rendered.contains("working=1"));
    assert!(!rendered.contains("merged"));
}

#[test]
fn workers_by_repo_names_the_repo_not_its_absolute_path() {
    let ledger = Ledger {
        entries: vec![LedgerEntry {
            task_id: "task".into(),
            dropr_task_id: None,
            display_id: "#1".into(),
            repo: "/Users/operator/repos/robco".into(),
            agent_id: "agent".into(),
            branch: "branch".into(),
            phase: LedgerPhase::Working,
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
        }],
        ..Ledger::default()
    };
    let mut repo = crate::discover::repo_node("/Users/operator/repos/robco".into(), false);
    repo.name = "robco".into();
    let registry = Registry {
        version: 1,
        repos: vec![repo],
    };
    let mut lines = Vec::new();
    append_ledger(&mut lines, &ledger, &[], &registry);
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("workers by repo"));
    assert!(rendered.contains("robco=1"));
    assert!(!rendered.contains("/Users/operator"));
}

#[test]
fn primary_holder_names_the_repo_and_the_task() {
    let ledger = Ledger {
        entries: vec![LedgerEntry {
            task_id: "task".into(),
            dropr_task_id: None,
            display_id: "#452".into(),
            repo: "/Users/operator/repos/robco".into(),
            agent_id: "agent".into(),
            branch: "branch".into(),
            phase: LedgerPhase::Working,
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
        }],
        ..Ledger::default()
    };
    let mut repo = crate::discover::repo_node("/Users/operator/repos/robco".into(), false);
    repo.name = "robco".into();
    let registry = Registry {
        version: 1,
        repos: vec![repo],
    };
    let mut lines = Vec::new();
    append_ledger(&mut lines, &ledger, &[], &registry);
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("primary holder"));
    assert!(rendered.contains("robco=#452"));
    assert!(!rendered.contains("/Users/operator"));
}

#[test]
fn inbox_and_discord_rows_agree_on_their_left_edge() {
    // dropr:497 — a Discord channel row used to render two columns further
    // right than an Inbox row nested the same way, because the two row
    // builders disagreed about how many columns their own marker took. Both
    // must start their content at `ROW_LEFT_EDGE`, no matter what either one
    // draws after that.
    let mut app = test_app();
    app.overseer_inbox = vec![crate::ui::inbox::InboxItem {
        kind: crate::ui::inbox::InboxKind::Escalation,
        repo: None,
        target_session: None,
        target_id: "task-1".into(),
        label: "task-1".into(),
        detail: "needs user".into(),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }];
    let inbox_line = inbox_rows::detail_lines(&app)[0].to_string();
    assert_eq!(
        inbox_line.len() - inbox_line.trim_start().len(),
        ROW_LEFT_EDGE,
        "inbox row: {inbox_line:?}"
    );

    let mut channels = crate::overseer::discord_channels::DiscordChannels::default();
    channels.channels.insert(
        "c1".into(),
        crate::overseer::discord_channels::ChannelAgent {
            first_seen_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
            turn_count: 1,
            status: crate::overseer::discord_channels::ChannelAgentStatus::Failed,
            last_error: Some("session timed out".into()),
            history: Vec::new(),
            channel_name: None,
        },
    );
    app.overseer_snapshot.discord_channels = channels;
    let discord_lines = discord_agents::detail_lines(&app);

    let channel_line = discord_lines[0].to_string();
    assert_eq!(
        channel_line.len() - channel_line.trim_start().len(),
        ROW_LEFT_EDGE,
        "discord row: {channel_line:?}"
    );

    // The error sub-line carries no marker of its own; it must still land on
    // the same column as the channel label above it.
    let error_line = discord_lines[1].to_string();
    assert_eq!(
        error_line.len() - error_line.trim_start().len(),
        ROW_LEFT_EDGE,
        "discord error row: {error_line:?}"
    );
}
