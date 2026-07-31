//! The sidebar layout the operator arranged, persisted to `~/.robco/ui-state.json`.
//!
//! Kept out of `state.json` — which `Registry::merge_discovered` rebuilds in
//! discovery order on every refresh — and out of `config.json`, which holds
//! settings the operator edits by hand. A third file keeps the ownership honest.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use crate::{Result, config::ui_state_path, model::OverseerCategory};

/// Every entry is keyed by a stable identity — a canonical repo path or an
/// agent worktree path — never by a position, because the registry is reordered
/// under the UI by discovery.
///
/// Sets are `BTreeSet` rather than `HashSet` so the file's key order does not
/// churn between writes and a diff of it stays readable.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::ui) struct UiState {
    /// Repos whose agent rows are collapsed. Stored as the exception rather
    /// than the rule so a repo this file has never seen — a freshly discovered
    /// one — starts expanded, matching how a scan treats it.
    pub(in crate::ui) collapsed_repos: BTreeSet<String>,
    /// Agents whose child-worktree rows are expanded. The in-memory form is
    /// already path-keyed, so it persists as-is.
    pub(in crate::ui) expanded_children: BTreeSet<String>,
    pub(in crate::ui) other_collapsed: bool,
    pub(in crate::ui) orphans_collapsed: bool,
    /// Labels of the expanded OVERSEER categories. Labels rather than indices:
    /// an index would silently re-point if the category list ever changes.
    pub(in crate::ui) expanded_overseer_categories: BTreeSet<String>,
    /// Canonical repo paths in the order the operator arranged them. Repos
    /// absent from the list sort after every listed one. Reserved here so the
    /// reorder binding does not have to migrate the file format.
    pub(in crate::ui) project_order: Vec<String>,
}

impl UiState {
    /// Per-repo expanded flags for `repos`, in registry order. Absence from the
    /// saved set means expanded, so a repo this file has never seen starts open
    /// — the same default a fresh discovery gives it.
    pub(in crate::ui) fn repo_expanded(&self, repos: &[crate::model::RepoNode]) -> Vec<bool> {
        repos
            .iter()
            .map(|repo| {
                !self
                    .collapsed_repos
                    .contains(&crate::ui::actions::discovery::path_key(&repo.path))
            })
            .collect()
    }

    /// Expanded flags for the OVERSEER categories, indexed by
    /// [`OverseerCategory::index`] — the order `App::overseer_expanded` is
    /// indexed in. Only [`OverseerCategory::ALL`] labels are read back: a label
    /// the current category list no longer carries top-level (a stale `Ledger`
    /// or `Decisions` from before dropr:378 folded them under `Details`) is
    /// silently ignored rather than a crash, and the next toggle rewrites the
    /// file without it.
    pub(in crate::ui) fn overseer_expanded(&self) -> [bool; OverseerCategory::COUNT] {
        let mut flags = [false; OverseerCategory::COUNT];
        for category in OverseerCategory::ALL {
            flags[category.index()] = self.expanded_overseer_categories.contains(category.label());
        }
        flags
    }
}

/// Owns the on-disk copy of [`UiState`]. A store with no path keeps its state
/// in memory and never touches the operator's home directory — that is what
/// tests and any headless construction get, so a test run cannot rewrite the
/// sidebar of the machine it happens to be running on.
pub(in crate::ui) struct UiStateStore {
    path: Option<PathBuf>,
    state: UiState,
}

impl UiStateStore {
    /// Read `~/.robco/ui-state.json`. A missing, unreadable, or corrupt file
    /// degrades to defaults: this is cosmetic state, never a launch blocker.
    /// A corrupt file is left in place rather than deleted — the next toggle
    /// overwrites it, and until then it stays inspectable.
    pub(in crate::ui) fn load() -> Self {
        match ui_state_path() {
            Ok(path) => {
                let state = read_at(&path);
                Self {
                    path: Some(path),
                    state,
                }
            }
            Err(_) => Self::in_memory(UiState::default()),
        }
    }

    /// A store that reads and writes nothing.
    pub(in crate::ui) fn in_memory(state: UiState) -> Self {
        Self { path: None, state }
    }

    pub(in crate::ui) fn state(&self) -> &UiState {
        &self.state
    }

    /// Mutate the state and write it back, unless the edit left it unchanged.
    ///
    /// A toggle is a discrete user action, so a straight write per real change
    /// is cheap and needs no debounce. An edit that changes nothing is not a
    /// user action at all, though, and several callers produce them routinely:
    /// `h` on an already-collapsed header re-asserts the same flag on every
    /// keypress, and the setters that rewrite a whole set recompute an
    /// identical one whenever the toggled entry was not in it. Comparing here
    /// rather than in each caller keeps the guard in one place — the store is
    /// what knows whether the file would actually differ.
    ///
    /// Persisting is best-effort either way: a write that fails costs a layout
    /// tweak, which is not worth interrupting the operator over.
    pub(in crate::ui) fn update(&mut self, edit: impl FnOnce(&mut UiState)) {
        let mut next = self.state.clone();
        edit(&mut next);
        if next == self.state {
            return;
        }
        self.state = next;
        if let Some(path) = &self.path {
            let _ = write_at(path, &self.state);
        }
    }
}

fn read_at(path: &Path) -> UiState {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Atomic temp-file-plus-rename, the same shape `Config::save_at` uses, so a
/// crash mid-write leaves the previous layout intact.
///
/// Writes are last-writer-wins with no cross-process lock. A single TUI owns
/// this file; two instances would each be describing their own sidebar anyway,
/// so the loser of a race forfeits nothing but its own tweak — cheaper than the
/// `fd_lock` dance `Registry` needs for state the daemon also writes.
fn write_at(path: &Path, state: &UiState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(state)?;
    let temp_path = path.with_extension(format!("json.{}.tmp", nanoid!()));
    let written = fs::write(&temp_path, raw).and_then(|()| fs::rename(&temp_path, path));
    if let Err(error) = written {
        let _ = fs::remove_file(temp_path);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "ui_state_tests.rs"]
mod tests;

#[cfg(test)]
impl UiStateStore {
    /// A store bound to `path`, for exercising the on-disk round trip without
    /// reaching for the real home directory.
    pub(in crate::ui) fn at(path: PathBuf) -> Self {
        let state = read_at(&path);
        Self {
            path: Some(path),
            state,
        }
    }
}
