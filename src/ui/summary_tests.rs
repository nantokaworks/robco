use super::*;
use crate::dropr::{DroprTaskCandidate, DroprTaskFetch, DroprWorkspace};

fn repo(dropr: Option<DroprWorkspace>, dropr_tasks: DroprTaskFetch) -> RepoNode {
    RepoNode {
        path: "/repo".into(),
        name: "repo".into(),
        remote_url: None,
        pinned: false,
        management: crate::model::ManagementMode::Auto,
        agents: Vec::new(),
        dropr,
        dropr_tasks,
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
    }
}

fn workspace() -> DroprWorkspace {
    DroprWorkspace {
        id: "11CZPXW".into(),
        name: "robco".into(),
        kind: "materialised".into(),
        repo_url: "https://github.com/nantokaworks/robco.git".into(),
    }
}

fn answered(tasks: Vec<DroprTaskCandidate>) -> DroprTaskFetch {
    DroprTaskFetch {
        tasks,
        problems: Vec::new(),
        answered: true,
    }
}

fn rendered(repo: &RepoNode) -> Vec<String> {
    rendered_with(repo, &Ledger::default())
}

fn rendered_with(repo: &RepoNode, ledger: &Ledger) -> Vec<String> {
    let (_, text) = repo_summary(
        repo,
        std::path::Path::new("/repos"),
        ledger,
        &crate::overseer::other_prs::OtherPrs::default(),
        40,
        crate::locale::Locale::En,
    );
    text.lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}

/// The defect: an unlinked repo dropped the DROPR block entirely, which looks
/// exactly like a linked repo whose board happens to be empty.
#[test]
fn a_repo_with_no_workspace_says_so_rather_than_rendering_nothing() {
    let lines = rendered(&repo(None, DroprTaskFetch::default()));

    assert!(lines.iter().any(|line| line == "DROPR"));
    assert!(
        lines
            .iter()
            .any(|line| line == "no workspace resolved for this repo, so no tasks can be listed")
    );
}

#[test]
fn a_linked_repo_names_its_workspace_and_lists_its_tasks() {
    let tasks = answered(vec![DroprTaskCandidate {
        display_id: "#250".into(),
        title: "Accept a merge verdict".into(),
        priority: "high".into(),
        status: "open".into(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
    }]);

    let lines = rendered(&repo(Some(workspace()), tasks));
    assert!(lines.iter().any(|line| line == "id: 11CZPXW"));
    assert!(lines.iter().any(|line| line == "next tasks"));
    assert!(
        lines
            .iter()
            .any(|line| line == "#250  Accept a merge verdict")
    );
    assert!(
        !lines
            .iter()
            .any(|line| line == "no workspace resolved for this repo, so no tasks can be listed")
    );
}

/// The summary reads the history off the selected repository's own path, so a
/// ledger entry that repository settled has to reach the pane.
#[test]
fn a_repo_summary_lists_the_overseer_entries_that_repo_settled() {
    let settled = crate::overseer::ledger::Ledger {
        entries: vec![crate::overseer::ledger::LedgerEntry {
            task_id: "task-263".into(),
            display_id: "#263".into(),
            repo: "/repo".into(),
            agent_id: "worker".into(),
            branch: "branch".into(),
            phase: crate::overseer::ledger::LedgerPhase::Merged,
            dispatched_at: chrono::Utc::now(),
            settled_at: None,
            retries: 0,
            pr_url: Some("https://github.com/nantokaworks/robco/pull/199".into()),
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
            worker_escalated: false,
            operator_override: None,
        }],
        ..Default::default()
    };

    let lines = rendered_with(
        &repo(Some(workspace()), DroprTaskFetch::default()),
        &settled,
    );
    assert!(lines.iter().any(|line| line == "HISTORY"));
    assert!(
        lines.iter().any(|line| line.contains("#263")
            && line.contains("merged")
            && line.contains("PR #199"))
    );
}

/// A repo whose workspace is virtual is skipped by the dispatch loop, and the
/// pane — not the decision log — is where that steady state must be visible.
#[test]
fn a_virtual_workspace_repo_says_dispatch_is_skipped() {
    let mut virtual_workspace = workspace();
    virtual_workspace.kind = "virtual".into();
    let lines = rendered(&repo(Some(virtual_workspace), DroprTaskFetch::default()));

    assert!(lines.iter().any(|line| line == "kind: virtual"));
    assert!(lines.iter().any(|line| {
        line == "workspace is not materialised, so the overseer does not dispatch tasks for this repo"
    }));

    // A materialised workspace must not carry the notice.
    let lines = rendered(&repo(Some(workspace()), DroprTaskFetch::default()));
    assert!(!lines.iter().any(|line| {
        line == "workspace is not materialised, so the overseer does not dispatch tasks for this repo"
    }));
}

/// A linked repo whose fetch failed must not read as an unlinked one, nor as an
/// empty board: the failure is its own state.
#[test]
fn a_linked_repo_whose_fetch_failed_reports_the_failure() {
    let lines = rendered(&repo(
        Some(workspace()),
        DroprTaskFetch::failed("root tasks: dropr refused: Not found"),
    ));

    assert!(lines.iter().any(|line| line == "id: 11CZPXW"));
    assert!(lines.iter().any(|line| line == "tasks unavailable"));
    assert!(
        lines
            .iter()
            .any(|line| line == "! root tasks: dropr refused: Not found")
    );
}

/// A primary checkout whose local `main` could not be fast-forwarded must
/// not sit silently behind `origin/main`; the summary pane is where the
/// drift becomes visible.
#[test]
fn a_repo_behind_origin_main_shows_the_drift() {
    let mut node = repo(None, DroprTaskFetch::default());
    node.main_behind_origin = Some(3);

    let lines = rendered(&node);

    assert!(
        lines
            .iter()
            .any(|line| line == "main behind origin/main by 3")
    );
}

/// A repo whose local `main` is caught up carries no drift line at all — not
/// even an empty one, since blank chrome is as much a false signal as a wrong
/// count.
#[test]
fn a_repo_caught_up_with_origin_main_shows_no_drift_line() {
    let lines = rendered(&repo(None, DroprTaskFetch::default()));

    assert!(!lines.iter().any(|line| line.starts_with("main behind")));
}
