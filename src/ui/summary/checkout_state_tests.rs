use super::*;

fn repo(checkout_state: Option<CheckoutState>) -> RepoNode {
    RepoNode {
        path: "/repo".into(),
        name: "repo".into(),
        remote_url: None,
        pinned: false,
        management: crate::model::ManagementMode::Auto,
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
    let lines = rendered(Some(CheckoutState::Detached));

    assert!(lines.iter().any(|line| line.contains("detached")));
}

/// A primary checkout on a branch other than `main` names that branch.
#[test]
fn a_primary_checkout_on_another_branch_names_it() {
    let lines = rendered(Some(CheckoutState::OtherBranch("wip".into())));

    assert!(lines.iter().any(|line| line.contains("wip")));
}

/// On `main`, no checkout-state warning renders at all.
#[test]
fn a_primary_checkout_on_main_shows_no_checkout_warning() {
    let lines = rendered(None);

    assert!(lines.is_empty());
}
