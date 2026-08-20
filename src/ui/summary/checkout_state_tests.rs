use super::*;

fn repo(checkout_state: Option<CheckoutState>) -> RepoNode {
    RepoNode {
        path: "/repo".into(),
        name: "repo".into(),
        remote_url: None,
        pinned: false,
        agents: Vec::new(),
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
        checkout_state,
    }
}

fn rendered(checkout_state: Option<CheckoutState>) -> Vec<String> {
    checkout_branch_warning(&repo(checkout_state), Locale::En)
        .iter()
        .map(|line| line.to_string())
        .collect()
}

/// A detached primary checkout must not sit silently unreported — this is
/// the fix for dropr:429: `ready` skips releases with an opaque reason and
/// `git pull` fails, both invisibly, until this line names the state.
#[test]
fn a_detached_primary_checkout_shows_a_warning() {
    let lines = rendered(Some(CheckoutState::Detached {
        default_branch: "main".into(),
    }));

    assert!(lines.iter().any(|line| line.contains("detached")));
}

/// A primary checkout on a branch other than the default names both.
#[test]
fn a_primary_checkout_on_another_branch_names_it() {
    let lines = rendered(Some(CheckoutState::OtherBranch {
        current: "wip".into(),
        default_branch: "main".into(),
    }));

    assert!(lines.iter().any(|line| line.contains("wip")));
}

/// dropr:503 — the same warning follows a repository whose default branch
/// is `master`, naming `master` rather than a hardcoded `main`.
#[test]
fn a_primary_checkout_on_another_branch_names_a_master_default() {
    let lines = rendered(Some(CheckoutState::OtherBranch {
        current: "wip".into(),
        default_branch: "master".into(),
    }));

    assert!(lines.iter().any(|line| line.contains("wip")));
    assert!(lines.iter().any(|line| line.contains("master")));
    assert!(lines.iter().all(|line| !line.contains("main")));
}

/// On the default branch, no checkout-state warning renders at all.
#[test]
fn a_primary_checkout_on_the_default_branch_shows_no_checkout_warning() {
    let lines = rendered(None);

    assert!(lines.is_empty());
}

/// dropr:503 — an unresolved default branch must warn the operator to fix
/// `origin/HEAD`, never silently assume `main`.
#[test]
fn an_unresolved_default_branch_shows_a_warning() {
    let lines = rendered(Some(CheckoutState::DefaultBranchUnknown));

    assert!(lines.iter().any(|line| line.contains("default branch")));
}
