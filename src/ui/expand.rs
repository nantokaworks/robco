//! Every operator-driven expand / collapse in the sidebar, and the write-through
//! that makes it outlive the process.
//!
//! These are the only writers of the flags [`crate::ui::ui_state::UiState`]
//! persists. A discovery re-key (`registry_write`, `background_refresh`) moves
//! the same flags onto reordered rows without going through here on purpose: it
//! is restoring a choice, not making one, so it must not rewrite the file.

use crate::model::OverseerCategory;

use super::{App, actions::discovery::path_key};

impl App {
    pub(in crate::ui) fn overseer_category_expanded(&self, category: OverseerCategory) -> bool {
        self.overseer_expanded[category.index()]
    }

    /// Expand or collapse one repo's agent rows.
    pub(in crate::ui) fn set_repo_expanded(&mut self, repo: usize, expanded: bool) {
        let Some(flag) = self.expanded.get_mut(repo) else {
            return;
        };
        if *flag == expanded {
            return;
        }
        *flag = expanded;
        self.persist_expanded_repos();
    }

    /// Rewrite the saved collapse set from what is currently on screen. Deriving
    /// it wholesale rather than patching one key in also prunes repos that have
    /// since left the registry, so a stale entry cannot re-collapse a row that
    /// comes back under the same path.
    fn persist_expanded_repos(&mut self) {
        let collapsed = self
            .registry
            .repos
            .iter()
            .enumerate()
            .filter(|(idx, _)| !self.expanded.get(*idx).copied().unwrap_or(true))
            .map(|(_, repo)| path_key(&repo.path))
            .collect();
        self.ui_state
            .update(|state| state.collapsed_repos = collapsed);
    }

    pub(in crate::ui) fn set_agent_children_expanded(
        &mut self,
        repo: usize,
        agent: usize,
        expanded: bool,
    ) {
        self.prune_expanded_children();
        let key = self.agent_children_key(repo, agent);
        if expanded {
            self.expanded_children.insert(key);
        } else {
            self.expanded_children.remove(&key);
        }
        // Persisted after the prune above, so the saved set never carries a key
        // for an agent the registry no longer has.
        let keys = self.expanded_children.iter().cloned().collect();
        self.ui_state
            .update(|state| state.expanded_children = keys);
        self.clamp_selection();
    }

    pub(in crate::ui) fn set_other_collapsed(&mut self, collapsed: bool) {
        self.other_collapsed = collapsed;
        self.ui_state
            .update(|state| state.other_collapsed = collapsed);
        self.clamp_selection();
    }

    pub(in crate::ui) fn set_orphans_collapsed(&mut self, collapsed: bool) {
        self.orphans_collapsed = collapsed;
        self.ui_state
            .update(|state| state.orphans_collapsed = collapsed);
        self.clamp_selection();
    }

    pub(in crate::ui) fn set_overseer_category_expanded(
        &mut self,
        category: OverseerCategory,
        expanded: bool,
    ) {
        self.overseer_expanded[category.index()] = expanded;
        // Labels rather than indices: the file has to survive a change to the
        // category list, and an index would silently re-point instead.
        let expanded_labels = OverseerCategory::ALL
            .into_iter()
            .filter(|category| self.overseer_expanded[category.index()])
            .map(|category| category.label().to_string())
            .collect();
        self.ui_state
            .update(|state| state.expanded_overseer_categories = expanded_labels);
        self.clamp_selection();
    }

    pub(in crate::ui) fn toggle_overseer_category(&mut self, category: OverseerCategory) {
        let expanded = !self.overseer_category_expanded(category);
        self.set_overseer_category_expanded(category, expanded);
    }
}
