//! Operator-chosen PROJECTS order, and the `Shift+Up` / `Shift+Down` binding
//! that sets it.
//!
//! Discovery sorts repos alphabetically (`discover::discover_all`) and
//! `Registry::merge_discovered` rebuilds the list in that order on every
//! refresh, so the chosen order cannot live in `state.json`. It is a UI-side
//! sort applied on top of the registry instead: `RepoNode` is never mutated,
//! `state.json` stays discovery-owned, and a refresh has nothing to overwrite.

use std::collections::HashMap;

use crate::model::Selection;

use super::{App, actions::discovery::path_key};

impl App {
    /// Sort one section's registry indices into the saved order.
    ///
    /// Repos the saved order does not name sort **after** every repo it does,
    /// alphabetically among themselves — the order discovery would have given
    /// them. So a newly discovered repo always appears, in a defined place, and
    /// never displaces a row the operator positioned by hand.
    ///
    /// A saved entry for a repo that is no longer in the registry simply never
    /// matches, so it cannot resurrect a phantom row.
    pub(in crate::ui) fn in_saved_order(&self, indices: Vec<usize>) -> Vec<usize> {
        let positions: HashMap<&str, usize> = self
            .ui_state
            .state()
            .project_order
            .iter()
            .enumerate()
            .map(|(position, key)| (key.as_str(), position))
            .collect();
        // Ranked up front rather than inside the comparator: `path_key`
        // canonicalizes, so ranking per comparison would put a filesystem call
        // on a path `visible()` walks on every draw.
        let mut ranked = indices
            .into_iter()
            .map(|idx| {
                let key = path_key(&self.registry.repos[idx].path);
                let rank = positions.get(key.as_str()).map_or((1, 0), |at| (0, *at));
                (rank, idx)
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(a_rank, a), (b_rank, b)| {
            a_rank.cmp(b_rank).then_with(|| {
                let (a, b) = (&self.registry.repos[*a], &self.registry.repos[*b]);
                a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path))
            })
        });
        ranked.into_iter().map(|(_, idx)| idx).collect()
    }

    /// Move the selected project row one slot within its own section.
    ///
    /// A no-op unless a repo row is selected: agent rows, the section headers,
    /// and the OVERSEER rows keep whatever the unmodified arrows do. Moving off
    /// either end is a no-op too, never a wrap.
    pub(in crate::ui) fn move_selected_repo(&mut self, delta: isize) {
        let Some(Selection::Repo(repo)) = self.selected_item() else {
            return;
        };
        // `visible()` emits local repos, then the "other locations" section,
        // then orphans. A repo moves among its own section's rows only, so a
        // move cannot smuggle it across a section boundary.
        let local = self.repo_is_local(&self.registry.repos[repo]);
        let mut section = if local {
            self.local_repos()
        } else {
            self.other_location_repos()
        };

        let Some(at) = section.iter().position(|idx| *idx == repo) else {
            return;
        };
        let Some(target) = at.checked_add_signed(delta) else {
            return;
        };
        if target >= section.len() {
            return;
        }
        section.swap(at, target);

        // `App::selected` is a flat index into a list rebuilt from scratch, so
        // the cursor is re-anchored on the row's identity rather than moved by
        // arithmetic — the moved row may have carried expanded agent rows with
        // it, and those change how far the index has to travel.
        let identity = self.item_key(Selection::Repo(repo));
        self.persist_project_order(local, section);
        self.restore_selection(Some(identity));
    }

    /// Write both sections' orders out, with `moved` standing in for whichever
    /// one was just reordered. Writing the whole sidebar rather than patching
    /// one entry also prunes repos that have left the registry, and settles the
    /// order of every repo that was until now merely alphabetical.
    fn persist_project_order(&mut self, moved_is_local: bool, moved: Vec<usize>) {
        let (local, others) = if moved_is_local {
            (moved, self.other_location_repos())
        } else {
            (self.local_repos(), moved)
        };
        let order = local
            .into_iter()
            .chain(others)
            .map(|idx| path_key(&self.registry.repos[idx].path))
            .collect();
        self.ui_state.update(|state| state.project_order = order);
    }
}

#[cfg(test)]
#[path = "reorder_tests.rs"]
mod tests;
