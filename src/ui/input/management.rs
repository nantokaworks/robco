use std::path::Path;

use crate::{
    Result,
    model::{AgentNode, ManagementMode, Selection},
    overseer::is_overseer_child,
    registry::Registry,
};

use super::{App, Mode};

pub(super) fn enroll_selected(app: &mut App) -> Result<()> {
    let Some(Selection::Agent { repo, agent }) = app.selected_item() else {
        app.show_message("e: select a worktree to enroll into overseer management");
        return Ok(());
    };
    let repo_path = app.registry.repos[repo].path.clone();
    let agent_id = app.registry.repos[repo].agents[agent].id.clone();
    let mut outcome = EnrollOutcome::NotFound;
    app.registry = Registry::locked_update(|registry| {
        if let Some(worker) = registry
            .repos
            .iter_mut()
            .find(|repo| repo.path == repo_path)
            .and_then(|repo| repo.agents.iter_mut().find(|agent| agent.id == agent_id))
        {
            outcome = enroll(worker);
        }
    })?;
    app.show_message(match outcome {
        EnrollOutcome::Enrolled => "enrolled into overseer management (auto)",
        EnrollOutcome::AlreadyManaged => "e: already overseer-managed",
        EnrollOutcome::NotFound => "e: selected worktree was not found",
    });
    Ok(())
}

pub(super) fn confirm_exclude_selected(app: &mut App) {
    let Some(Selection::Agent { repo, agent }) = app.selected_item() else {
        app.show_message("E: select an overseer worktree to exclude");
        return;
    };
    if !is_overseer_child(
        app.registry.repos[repo].agents[agent]
            .parent_agent_id
            .as_deref(),
    ) {
        app.show_message("E: only overseer-managed worktrees can be excluded");
        return;
    }
    let repo = &app.registry.repos[repo];
    let agent = &repo.agents[agent];
    app.mode = Mode::ConfirmOverseerExclude {
        repo_path: repo.path.clone(),
        agent_id: agent.id.clone(),
        title: agent.title.clone(),
    };
}

pub(super) fn exclude_selected(app: &mut App, repo_path: &Path, agent_id: &str) -> Result<()> {
    let mut outcome = ExcludeOutcome::NotFound;
    app.registry = Registry::locked_update(|registry| {
        if let Some(worker) = registry
            .repos
            .iter_mut()
            .find(|repo| repo.path == repo_path)
            .and_then(|repo| repo.agents.iter_mut().find(|agent| agent.id == agent_id))
        {
            outcome = exclude(worker);
        }
    })?;
    app.show_message(match outcome {
        ExcludeOutcome::Excluded => "excluded from overseer management (worker left running)",
        ExcludeOutcome::NotOverseerChild => "E: only overseer-managed worktrees can be excluded",
        ExcludeOutcome::NotFound => "E: selected worktree was not found",
    });
    Ok(())
}

pub(super) fn toggle_selected(app: &mut App) -> Result<()> {
    let Some(Selection::Agent { repo, agent }) = app.selected_item() else {
        app.show_message("g: select an overseer worker to toggle auto/manual");
        return Ok(());
    };
    let repo_path = app.registry.repos[repo].path.clone();
    let agent_id = app.registry.repos[repo].agents[agent].id.clone();
    let mut toggled = None;
    app.registry = Registry::locked_update(|registry| {
        let Some(worker) = registry
            .repos
            .iter_mut()
            .find(|repo| repo.path == repo_path)
            .and_then(|repo| repo.agents.iter_mut().find(|agent| agent.id == agent_id))
        else {
            return;
        };
        if toggle_mode(worker.parent_agent_id.as_deref(), &mut worker.management) {
            toggled = Some(worker.management);
        }
    })?;
    if let Some(mode) = toggled {
        app.show_message(format!(
            "overseer management: {}",
            format!("{mode:?}").to_ascii_lowercase()
        ));
    } else {
        app.show_message("g: only overseer-managed workers toggle auto/manual");
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum EnrollOutcome {
    Enrolled,
    AlreadyManaged,
    NotFound,
}

fn enroll(worker: &mut AgentNode) -> EnrollOutcome {
    if is_overseer_child(worker.parent_agent_id.as_deref()) {
        return EnrollOutcome::AlreadyManaged;
    }
    worker.parent_agent_id = Some(crate::overseer::OVERSEER_AGENT_ID.to_string());
    worker.management = ManagementMode::Auto;
    EnrollOutcome::Enrolled
}

#[derive(Debug, PartialEq, Eq)]
enum ExcludeOutcome {
    Excluded,
    NotOverseerChild,
    NotFound,
}

fn exclude(worker: &mut AgentNode) -> ExcludeOutcome {
    if !is_overseer_child(worker.parent_agent_id.as_deref()) {
        return ExcludeOutcome::NotOverseerChild;
    }
    worker.parent_agent_id = None;
    ExcludeOutcome::Excluded
}

fn toggle_mode(parent: Option<&str>, mode: &mut ManagementMode) -> bool {
    if !is_overseer_child(parent) {
        return false;
    }
    *mode = mode.toggled();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, model::Status, registry::Registry};
    use chrono::Local;

    fn worker(parent: Option<&str>, management: ManagementMode) -> AgentNode {
        AgentNode {
            id: "worker-1".into(),
            parent_agent_id: parent.map(str::to_string),
            management,
            title: "worker".into(),
            worktree_path: "/tmp/worker-1".into(),
            branch: "worker-1".into(),
            base_commit: "abc123".into(),
            program: "claude".into(),
            claude_session_id: None,
            profile: None,
            tmux_session: "worker-1".into(),
            created_at: Local::now(),
            updated_at: Local::now(),
            status: Status::Idle,
            worktree_missing: false,
            merge_error: None,
            last_capture: None,
            last_spinner: None,
            last_change_at: None,
            last_auto_accept_at: None,
            shell_working: false,
            pane_pid: None,
            tracked_command: None,
            subagents: vec![],
            children: vec![],
        }
    }

    #[test]
    fn toggle_on_non_worker_selection_explains_scope() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.overseer_visible = true;
        app.selected = 0; // OVERSEER header row — not a worker (Selection::Agent).

        toggle_selected(&mut app).unwrap();

        assert!(
            app.message
                .as_ref()
                .is_some_and(|(text, _)| text.contains("overseer worker")),
            "expected a scope hint message, got {:?}",
            app.message
        );
    }

    #[test]
    fn only_overseer_workers_toggle() {
        let mut mode = ManagementMode::Auto;
        assert!(toggle_mode(Some("overseer"), &mut mode));
        assert_eq!(mode, ManagementMode::Manual);
        assert!(toggle_mode(Some("chief"), &mut mode));
        assert_eq!(mode, ManagementMode::Auto);
        assert!(!toggle_mode(None, &mut mode));
        assert_eq!(mode, ManagementMode::Auto);
    }

    #[test]
    fn enroll_sets_overseer_parent_and_auto_management() {
        let mut worker = worker(None, ManagementMode::Manual);

        assert_eq!(enroll(&mut worker), EnrollOutcome::Enrolled);

        assert_eq!(worker.parent_agent_id.as_deref(), Some("overseer"));
        assert_eq!(worker.management, ManagementMode::Auto);
    }

    #[test]
    fn enroll_preserves_management_when_already_managed() {
        let mut worker = worker(Some("overseer"), ManagementMode::Manual);

        assert_eq!(enroll(&mut worker), EnrollOutcome::AlreadyManaged);

        assert_eq!(worker.parent_agent_id.as_deref(), Some("overseer"));
        assert_eq!(worker.management, ManagementMode::Manual);
    }

    #[test]
    fn exclude_clears_overseer_parent() {
        let mut worker = worker(Some("overseer"), ManagementMode::Manual);

        assert_eq!(exclude(&mut worker), ExcludeOutcome::Excluded);

        assert_eq!(worker.parent_agent_id, None);
        assert_eq!(worker.management, ManagementMode::Manual);
    }

    #[test]
    fn exclude_preserves_non_overseer_parent() {
        let mut worker = worker(Some("other-parent"), ManagementMode::Manual);

        assert_eq!(exclude(&mut worker), ExcludeOutcome::NotOverseerChild);

        assert_eq!(worker.parent_agent_id.as_deref(), Some("other-parent"));
    }
}
