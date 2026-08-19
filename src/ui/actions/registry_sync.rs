//! Reconcile the discovery snapshot against the agent rows stored on disk, and
//! carry the UI state that addresses those rows by position across the swap.

use std::path::PathBuf;

use crate::{
    model::RepoNode,
    registry::Registry,
    ui::{Mode, registry_write::carry_agent},
};

/// Replace each repo's agent rows with the ones the state file currently holds,
/// keeping this process's runtime-only state for the rows that survive.
///
/// The TUI reads `state.json` once at start-up and refreshes from its own
/// in-memory copy after that, so an agent the Overseer daemon removed during
/// post-merge cleanup stayed in the tree for the rest of the session — still
/// selectable, and still attachable, which spawned a fresh session in `$HOME`
/// because the worktree it named was gone. The stored registry is the only
/// process-wide record of which agents exist, so it decides membership here;
/// the in-memory row keeps everything marked `#[serde(skip)]`. That is the same
/// split `App::locked_registry_update` applies to a registry it just wrote.
///
/// A dropped row takes its runtime state with it, `merge_error` included, so
/// neither the `merge-failed` badge nor the per-repo count outlives the row it
/// described. If the row's tmux session outlives it, `orphans::discover_orphans`
/// — which runs later in the same capture, over the reconciled rows — reports it
/// under Orphans. Killing another process's session from a refresh pass is not
/// this function's call to make.
///
/// A repo absent from `stored` keeps its agents: discovery adopts repositories
/// and worktrees into memory before the save thread persists them, so an unstored
/// repo means "not written yet" rather than "removed".
pub(super) fn adopt_stored_agents(repos: &mut [RepoNode], stored: &Registry) {
    for repo in repos {
        let Some(source) = stored.repos.iter().find(|source| source.path == repo.path) else {
            continue;
        };
        let mut adopted = source.agents.clone();
        for agent in &mut adopted {
            if let Some(live) = repo.agents.iter().find(|live| live.id == agent.id) {
                carry_agent(live, agent);
            }
        }
        repo.agents = adopted;
    }
}

/// Identity of the agent an open confirmation dialog addresses, or `None` when
/// no index-keyed dialog is open — or when its indices already resolve to
/// nothing. Pair every call with [`restore_dialog_agent`] across the swap.
///
/// These dialogs store `(repo, agent)` as positions, and the draw path indexes
/// straight into the registry with them. A refresh that drops or reorders rows
/// underneath one would therefore either confirm against whatever agent slid
/// into that slot, or index past the end of the vector and panic mid-draw —
/// with the daemon's post-merge cleanup racing an operator's own merge dialog
/// being exactly the case that arises. `Mode::ConfirmKillOrphan` already stores
/// a session name for this reason; these variants keep their indices, so the
/// refresh rebuilds them instead.
pub(super) fn dialog_agent(mode: &Mode, repos: &[RepoNode]) -> Option<(PathBuf, String)> {
    let (repo, agent) = dialog_indices(mode)?;
    let repo = repos.get(repo)?;
    Some((repo.path.clone(), repo.agents.get(agent)?.id.clone()))
}

/// Re-point an open confirmation dialog at the row `anchor` names, closing the
/// dialog when that row is gone.
pub(super) fn restore_dialog_agent(
    mode: &mut Mode,
    repos: &[RepoNode],
    anchor: Option<(PathBuf, String)>,
) {
    if dialog_indices(mode).is_none() {
        return;
    }
    match anchor.and_then(|(path, id)| super::lifecycle::resolve_agent(repos, &path, &id)) {
        Some((repo, agent)) => set_dialog_indices(mode, repo, agent),
        None => *mode = Mode::Normal,
    }
}

fn dialog_indices(mode: &Mode) -> Option<(usize, usize)> {
    match mode {
        Mode::ConfirmKill { repo, agent }
        | Mode::ConfirmMerge { repo, agent, .. }
        | Mode::ConfirmCleanup { repo, agent }
        | Mode::ConfirmDeleteBranch { repo, agent } => Some((*repo, *agent)),
        _ => None,
    }
}

fn set_dialog_indices(mode: &mut Mode, repo: usize, agent: usize) {
    if let Mode::ConfirmKill {
        repo: dialog_repo,
        agent: dialog_agent,
    }
    | Mode::ConfirmMerge {
        repo: dialog_repo,
        agent: dialog_agent,
        ..
    }
    | Mode::ConfirmCleanup {
        repo: dialog_repo,
        agent: dialog_agent,
    }
    | Mode::ConfirmDeleteBranch {
        repo: dialog_repo,
        agent: dialog_agent,
    } = mode
    {
        *dialog_repo = repo;
        *dialog_agent = agent;
    }
}

#[cfg(test)]
#[path = "registry_sync_tests.rs"]
mod tests;
