use super::render::append_ledger;
use super::*;
use crate::locale::Locale;
use crate::overseer::autonomy::AutonomyLevel;
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
        task_number: None,
        worktree_path: format!("/tmp/{id}").into(),
        branch: id.into(),
        base_commit: String::new(),
        program: "codex".into(),
        claude_session_id: None,
        profile: None,
        tmux_session: id.into(),
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

fn management_app() -> App {
    let mut app = test_app();
    app.registry.repos.push(RepoNode {
        path: "/tmp/repo".into(),
        name: "repo".into(),
        remote_url: None,
        pinned: false,
        management: crate::model::ManagementMode::Auto,
        agents: vec![
            overseer_worker("auto-agent", "auto title", ManagementMode::Auto),
            overseer_worker("manual-agent", "manual title", ManagementMode::Manual),
        ],
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
            settled_at: None,
            retries: 0,
            pr_url: None,
            branch_updates: 0,
            merge_judge_primes: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_judge_fail_safes: 0,
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
        },
        LedgerEntry {
            task_id: "manual-task".into(),
            display_id: String::new(),
            repo: "repo".into(),
            agent_id: "manual-agent".into(),
            branch: "manual".into(),
            phase: LedgerPhase::Claimed,
            dispatched_at: Utc::now(),
            settled_at: None,
            retries: 0,
            pr_url: None,
            branch_updates: 0,
            merge_judge_primes: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_judge_fail_safes: 0,
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
    super::render::append_health(
        &mut lines,
        &config,
        &open,
        false,
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
    assert!(rendered.contains("circuit: OPEN"));
    assert!(rendered.contains("[R]"));
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
    super::render::append_health(
        &mut lines,
        &config,
        &closed,
        false,
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
    assert!(!rendered.contains("robco overseer set dispatch on"));
}

#[test]
fn health_flags_show_the_autonomy_level_and_warn_only_when_it_widens_the_envelope() {
    let render = |autonomy_level, auto_merge| {
        let config = OverseerConfig {
            autonomy_level,
            auto_merge,
            ..OverseerConfig::default()
        };
        let mut lines = Vec::new();
        super::render::append_health(
            &mut lines,
            &config,
            &Ledger::default(),
            true,
            None,
            None,
            None,
            Locale::En,
        );
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };

    for (level, label) in [
        (AutonomyLevel::ApprovalOnly, "approval_only"),
        (AutonomyLevel::Conservative, "conservative"),
        (AutonomyLevel::FullAuto, "full_auto"),
    ] {
        assert!(
            render(level, true).contains(&format!("autonomy: {label}")),
            "expected autonomy: {label}"
        );
    }

    assert!(
        render(AutonomyLevel::FullAuto, true).contains(crate::overseer::FULL_AUTO_ENVELOPE_HINT)
    );
    // The envelope only decides anything while the merge pass runs, so a level
    // no merge pass consults must not be reported as a loosened gate.
    assert!(
        !render(AutonomyLevel::FullAuto, false).contains(crate::overseer::FULL_AUTO_ENVELOPE_HINT)
    );
    assert!(
        !render(AutonomyLevel::Conservative, true)
            .contains(crate::overseer::FULL_AUTO_ENVELOPE_HINT)
    );

    let mut lines = Vec::new();
    let config = OverseerConfig {
        autonomy_level: AutonomyLevel::FullAuto,
        auto_merge: true,
        ..OverseerConfig::default()
    };
    super::render::append_health(
        &mut lines,
        &config,
        &Ledger::default(),
        true,
        None,
        None,
        None,
        Locale::En,
    );
    assert_eq!(
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content == "full_auto")
            .unwrap()
            .style
            .fg,
        Some(Color::Red)
    );
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
    let (summary, warn) = super::categories::health_summary_from(&config, &ledger, false, false);

    assert!(warn);
    assert!(summary.contains("STALE/OFFLINE"));
    assert!(summary.contains("circuit OPEN"));
    assert!(summary.contains("dispatch/no daemon"));
}

#[test]
fn health_frame_shows_merge_recovery_and_flags_it_when_it_cannot_fire() {
    let rendered = |config: &OverseerConfig| {
        let mut lines = Vec::new();
        super::render::append_health(
            &mut lines,
            config,
            &Ledger::default(),
            true,
            None,
            None,
            None,
            Locale::En,
        );
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
        &Ledger::default(),
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
        &Ledger::default(),
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
        &Ledger::default(),
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
    let (summary, warn) = super::categories::health_summary_from(
        &OverseerConfig::default(),
        &Ledger::default(),
        true,
        true,
    );

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
fn info_pane_reads_dispatch_from_snapshot_not_stale_config() {
    // Regression for #171: the Info pane must render overseer flags from the
    // disk-backed snapshot, not the in-memory `app.config` that only the `,`
    // settings editor refreshes. Simulate an `S` panic-stop landing on disk
    // (snapshot reloaded → dispatch off) while `app.config` is still stale "on".
    let mut app = test_app();
    app.config.overseer.dispatch_enabled = true;
    app.overseer_snapshot.overseer.dispatch_enabled = false;

    let rendered = category_text(&app, OverseerCategory::Health);

    assert!(rendered.contains("dispatch: off"));
    assert!(!rendered.contains("dispatch: on"));
}

#[test]
fn ledger_detail_shows_active_worker_management_counts() {
    let app = management_app();

    assert!(category_text(&app, OverseerCategory::Ledger).contains("management: auto=1, manual=1"));
}

#[test]
fn ledger_detail_shows_zero_manual_worker_count() {
    let mut app = management_app();
    app.overseer_snapshot
        .ledger
        .entries
        .retain(|entry| entry.agent_id == "auto-agent");

    assert!(category_text(&app, OverseerCategory::Ledger).contains("management: auto=1, manual=0"));
}

#[test]
fn ledger_detail_reports_the_pull_requests_management_is_withholding() {
    let mut app = management_app();
    // Nothing is being withheld until the merge pass says so, and the operator
    // should not read a manual worker that has not opened a pull request yet as
    // one whose merge is being declined.
    assert!(!category_text(&app, OverseerCategory::Ledger).contains("merge-eligible, manual"));

    let entry = app
        .overseer_snapshot
        .ledger
        .entries
        .iter_mut()
        .find(|entry| entry.agent_id == "manual-agent")
        .unwrap();
    entry.phase = LedgerPhase::PrOpened;
    entry.pr_url = Some("https://pr/1".into());
    entry.manual_merge_skip = Some("https://pr/1".into());

    assert!(category_text(&app, OverseerCategory::Ledger).contains("merge-eligible, manual: 1"));
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

    let rendered = category_text(&app, OverseerCategory::Ledger);
    assert!(rendered.contains("management: auto=1, manual=1"));
    assert_eq!(rendered.matches("worker #1: Auto").count(), 1);
    assert!(!rendered.contains("worker #duplicate"));
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
fn details_detail_combines_what_its_folded_children_carry() {
    // The Details row previews the ledger and the decisions in one read
    // (dropr:378): everything each folded child renders on its own must appear,
    // verbatim, in the combined detail.
    let app = management_app();
    let combined = category_text(&app, OverseerCategory::Details);
    for child in OverseerCategory::DETAILS_CHILDREN {
        let child_text = category_text(&app, child);
        assert!(
            combined.contains(&child_text),
            "{} detail missing from Details: {combined:?}",
            child.label()
        );
    }
}

#[test]
fn stale_dispatch_counter_renders_zero() {
    let today = Utc::now().date_naive();
    let mut ledger = Ledger::default();
    ledger.counters.date = today.pred_opt();
    ledger.counters.dispatched_today = 7;
    let mut lines = Vec::new();
    append_ledger(
        &mut lines,
        &OverseerConfig::default(),
        &ledger,
        &[],
        &[],
        &Registry::default(),
    );
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
        &[],
        &Registry::default(),
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
                settled_at: None,
                retries: 0,
                pr_url: None,
                branch_updates: 0,
                merge_judge_primes: 0,
                merge_recovery: Default::default(),
                merge_hold: Default::default(),
                manual_merge_skip: None,
                merge_judge_fail_safes: 0,
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
            },
            LedgerEntry {
                task_id: "terminal".into(),
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
                merge_judge_primes: 0,
                merge_recovery: Default::default(),
                merge_hold: Default::default(),
                manual_merge_skip: None,
                merge_judge_fail_safes: 0,
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
            },
        ],
        ..Ledger::default()
    };
    let mut lines = Vec::new();
    append_ledger(
        &mut lines,
        &OverseerConfig::default(),
        &ledger,
        &[],
        &[],
        &Registry::default(),
    );
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
            merge_judge_primes: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_judge_fail_safes: 0,
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
    append_ledger(
        &mut lines,
        &OverseerConfig::default(),
        &ledger,
        &[],
        &[],
        &registry,
    );
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
            merge_judge_primes: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_judge_fail_safes: 0,
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
    append_ledger(
        &mut lines,
        &OverseerConfig::default(),
        &ledger,
        &[],
        &[],
        &registry,
    );
    let rendered = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("primary holder"));
    assert!(rendered.contains("robco=#452"));
    assert!(!rendered.contains("/Users/operator"));
}
