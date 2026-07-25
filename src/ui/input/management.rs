use std::path::Path;

use crate::{
    Result,
    model::{AgentNode, ManagementMode, Selection},
    overseer::is_overseer_child,
};

use super::{App, Mode};

/// Where a worktree sits on the `g` cycle.
///
/// The position is a function of `parent_agent_id` and `management` together,
/// never of `management` alone: a worktree adopted from `worktree_root` is
/// persisted as `Manual` while nobody owns it, so reading the stored mode by
/// itself would place an unowned worktree mid-cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CycleStep {
    Unmanaged,
    Auto,
    Manual,
}

/// The step `worker` currently occupies, or `None` when it is off the cycle.
///
/// `parent_agent_id` is overloaded: it records overseer ownership *and* the
/// identity-tree parent `model::agent_order` draws from, which adoption can
/// recover from a live session's `ROBCO_PARENT_AGENT_ID`. Nothing persists what
/// the parent was before enrollment, so enrolling a worker that already belongs
/// to another agent would sever that link with no way to restore it on the way
/// back out. An overseer-managed worker is therefore defined as never also
/// being another agent's child, and `g` declines such a worker instead of
/// overwriting its parent.
fn cycle_step(worker: &AgentNode) -> Option<CycleStep> {
    match worker.parent_agent_id.as_deref() {
        None => Some(CycleStep::Unmanaged),
        Some(parent) if is_overseer_child(Some(parent)) => Some(match worker.management {
            ManagementMode::Auto => CycleStep::Auto,
            ManagementMode::Manual => CycleStep::Manual,
        }),
        Some(_) => None,
    }
}

/// Advance one step of `Unmanaged -> Auto -> Manual -> Unmanaged`, returning
/// the step landed on. `None` leaves the worker untouched (see [`cycle_step`]).
fn advance(worker: &mut AgentNode) -> Option<CycleStep> {
    Some(match cycle_step(worker)? {
        CycleStep::Unmanaged => {
            worker.parent_agent_id = Some(crate::overseer::OVERSEER_AGENT_ID.to_string());
            // An adopted worktree carries a stale `Manual`; overwrite rather
            // than preserve it, or enrollment would land on the wrong step.
            worker.management = ManagementMode::Auto;
            CycleStep::Auto
        }
        CycleStep::Auto => {
            worker.management = ManagementMode::Manual;
            CycleStep::Manual
        }
        CycleStep::Manual => {
            // Detaching only drops ownership; the worker and its tmux session
            // keep running. `management` stays Manual, but the daemon reads
            // ownership rather than mode: seeing the worker is no longer its
            // child, it drops any ledger entry it had for it instead of
            // freezing one for a worker it may no longer touch. See
            // `overseer::monitor::drop_detached`.
            worker.parent_agent_id = None;
            CycleStep::Unmanaged
        }
    })
}

#[derive(Debug, PartialEq, Eq)]
enum CycleOutcome {
    Moved(CycleStep),
    OtherParent,
    NotFound,
}

pub(super) fn cycle_selected(app: &mut App) -> Result<()> {
    match app.selected_item() {
        Some(Selection::Agent { repo, agent }) => cycle_worker(app, repo, agent),
        Some(Selection::Repo(repo)) => {
            confirm_bulk_toggle(app, repo);
            Ok(())
        }
        _ => {
            app.show_message("g: select a repo or a worktree to cycle overseer management");
            Ok(())
        }
    }
}

fn cycle_worker(app: &mut App, repo: usize, agent: usize) -> Result<()> {
    let repo_path = app.registry.repos[repo].path.clone();
    let agent_id = app.registry.repos[repo].agents[agent].id.clone();
    let mut outcome = CycleOutcome::NotFound;
    app.locked_registry_update(|registry| {
        let Some(worker) = registry
            .repos
            .iter_mut()
            .find(|repo| repo.path == repo_path)
            .and_then(|repo| repo.agents.iter_mut().find(|agent| agent.id == agent_id))
        else {
            return;
        };
        outcome = match advance(worker) {
            Some(step) => CycleOutcome::Moved(step),
            None => CycleOutcome::OtherParent,
        };
    })?;
    app.show_message(match outcome {
        CycleOutcome::Moved(CycleStep::Auto) => "overseer management: auto",
        CycleOutcome::Moved(CycleStep::Manual) => "overseer management: manual",
        CycleOutcome::Moved(CycleStep::Unmanaged) => {
            "detached from overseer management (worker left running)"
        }
        CycleOutcome::OtherParent => "g: this worktree belongs to another agent, not the overseer",
        CycleOutcome::NotFound => "g: selected worktree was not found",
    });
    Ok(())
}

fn confirm_bulk_toggle(app: &mut App, repo: usize) {
    let repo = &app.registry.repos[repo];
    let Some((target, count)) = bulk_target(&repo.agents) else {
        app.show_message("g: no worktrees under this repo for the overseer to manage");
        return;
    };
    app.mode = Mode::ConfirmOverseerBulkToggle {
        repo_path: repo.path.clone(),
        repo_name: repo.name.clone(),
        target,
        count,
    };
}

/// The mode a repo-wide toggle moves every worker to, plus how many of them it
/// would change. Biased toward standing the repo down: a single Auto worker
/// makes the whole repo Manual, and only a repo with no Auto worker left goes
/// to Auto — enrolling its unmanaged worktrees on the way there. The repo-level
/// action deliberately never un-enrolls; detaching stays a per-worktree
/// decision. That way one press always means "hands off this repo" unless there
/// is nothing left to stand down.
///
/// `None` when the repo holds no worktree the overseer may touch.
fn bulk_target(agents: &[AgentNode]) -> Option<(ManagementMode, usize)> {
    let (mut on_cycle, mut auto) = (0usize, 0usize);
    for step in agents.iter().filter_map(cycle_step) {
        on_cycle += 1;
        auto += usize::from(step == CycleStep::Auto);
    }
    match (on_cycle, auto) {
        (0, _) => None,
        (total, 0) => Some((ManagementMode::Auto, total)),
        _ => Some((ManagementMode::Manual, auto)),
    }
}

pub(super) fn bulk_toggle_repo(
    app: &mut App,
    repo_path: &Path,
    target: ManagementMode,
) -> Result<()> {
    let mut changed = 0usize;
    app.locked_registry_update(|registry| {
        let Some(repo) = registry
            .repos
            .iter_mut()
            .find(|repo| repo.path == repo_path)
        else {
            return;
        };
        for worker in &mut repo.agents {
            let Some(step) = cycle_step(worker) else {
                continue;
            };
            match target {
                // Standing down leaves unmanaged worktrees alone.
                ManagementMode::Manual if step == CycleStep::Auto => {
                    worker.management = ManagementMode::Manual;
                    changed += 1;
                }
                ManagementMode::Auto if step != CycleStep::Auto => {
                    if step == CycleStep::Unmanaged {
                        worker.parent_agent_id =
                            Some(crate::overseer::OVERSEER_AGENT_ID.to_string());
                    }
                    worker.management = ManagementMode::Auto;
                    changed += 1;
                }
                _ => {}
            }
        }
    })?;
    app.show_message(bulk_message(changed, target));
    Ok(())
}

fn bulk_message(changed: usize, target: ManagementMode) -> String {
    let action = bulk_action(target);
    match changed {
        0 => format!("g: no workers left to {action}"),
        1 => format!("1 worker {action}"),
        _ => format!("{changed} workers {action}"),
    }
}

/// Past-tense phrase naming what a repo-wide toggle does, shared by the confirm
/// dialog so the prompt and the result read the same way.
pub(in crate::ui) fn bulk_action(target: ManagementMode) -> &'static str {
    match target {
        ManagementMode::Auto => "put under overseer management (auto)",
        ManagementMode::Manual => "set to manual",
    }
}

#[cfg(test)]
#[path = "management_tests.rs"]
mod tests;
