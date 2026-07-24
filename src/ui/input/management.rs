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
    match app.selected_item() {
        Some(Selection::Agent { repo, agent }) => toggle_worker(app, repo, agent),
        Some(Selection::Repo(repo)) => {
            confirm_bulk_toggle(app, repo);
            Ok(())
        }
        _ => {
            app.show_message("g: select a repo or an overseer worker to toggle auto/manual");
            Ok(())
        }
    }
}

fn toggle_worker(app: &mut App, repo: usize, agent: usize) -> Result<()> {
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

fn confirm_bulk_toggle(app: &mut App, repo: usize) {
    let repo = &app.registry.repos[repo];
    let Some((target, count)) = bulk_target(&repo.agents) else {
        app.show_message("g: no overseer-managed workers under this repo");
        return;
    };
    app.mode = Mode::ConfirmOverseerBulkToggle {
        repo_path: repo.path.clone(),
        repo_name: repo.name.clone(),
        target,
        count,
    };
}

/// The mode a repo-wide toggle moves every overseer-managed worker to, plus how
/// many of them currently differ from it. Biased toward standing the repo down:
/// a single Auto worker makes the whole repo Manual, and only a repo that is
/// already fully Manual goes back to Auto. That way one press always means
/// "hands off this repo" unless there is nothing left to stand down.
///
/// `None` when the repo holds no overseer-managed workers at all.
fn bulk_target(agents: &[AgentNode]) -> Option<(ManagementMode, usize)> {
    let managed = agents
        .iter()
        .filter(|agent| is_overseer_child(agent.parent_agent_id.as_deref()));
    let (mut total, mut auto) = (0usize, 0usize);
    for agent in managed {
        total += 1;
        auto += usize::from(agent.management == ManagementMode::Auto);
    }
    match (total, auto) {
        (0, _) => None,
        (_, 0) => Some((ManagementMode::Auto, total)),
        _ => Some((ManagementMode::Manual, auto)),
    }
}

pub(super) fn bulk_toggle_repo(
    app: &mut App,
    repo_path: &Path,
    target: ManagementMode,
) -> Result<()> {
    let mut changed = 0usize;
    app.registry = Registry::locked_update(|registry| {
        let Some(repo) = registry
            .repos
            .iter_mut()
            .find(|repo| repo.path == repo_path)
        else {
            return;
        };
        for worker in &mut repo.agents {
            if is_overseer_child(worker.parent_agent_id.as_deref()) && worker.management != target {
                worker.management = target;
                changed += 1;
            }
        }
    })?;
    app.show_message(bulk_message(changed, target));
    Ok(())
}

fn bulk_message(changed: usize, target: ManagementMode) -> String {
    let mode = format!("{target:?}").to_ascii_lowercase();
    match changed {
        0 => format!("g: no workers left to set to {mode}"),
        1 => format!("1 worker set to {mode}"),
        _ => format!("{changed} workers set to {mode}"),
    }
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
#[path = "management_tests.rs"]
mod tests;
