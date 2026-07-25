use super::*;
use crate::dropr::{DroprTaskCandidate, DroprTaskFetch, DroprWorkspace};

fn repo(dropr: Option<DroprWorkspace>, dropr_tasks: DroprTaskFetch) -> RepoNode {
    RepoNode {
        path: "/repo".into(),
        name: "repo".into(),
        remote_url: None,
        pinned: false,
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
    let (_, text) = repo_summary(repo, std::path::Path::new("/repos"), 40);
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
