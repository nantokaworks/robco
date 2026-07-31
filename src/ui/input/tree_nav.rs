use crate::model::Selection;

use super::super::App;

impl App {
    pub(in crate::ui) fn agent_children_key(&self, repo: usize, agent: usize) -> String {
        crate::ui::actions::discovery::path_key(
            &self.registry.repos[repo].agents[agent].worktree_path,
        )
    }

    pub(in crate::ui) fn agent_children_expanded(&self, repo: usize, agent: usize) -> bool {
        self.expanded_children
            .contains(&self.agent_children_key(repo, agent))
    }

    pub(in crate::ui) fn prune_expanded_children(&mut self) {
        // Callers persist afterwards where the prune is part of a user-driven
        // toggle; see `crate::ui::expand`.
        let live = self
            .registry
            .repos
            .iter()
            .flat_map(|repo| &repo.agents)
            .map(|agent| crate::ui::actions::discovery::path_key(&agent.worktree_path))
            .collect::<std::collections::HashSet<_>>();
        self.expanded_children.retain(|key| live.contains(key));
    }

    pub(super) fn expand_selected_tree_item(&mut self) {
        match self.selected_item() {
            Some(Selection::OverseerCategory(category)) => {
                self.set_overseer_category_expanded(category, true);
            }
            Some(Selection::Repo(repo)) => self.set_repo_expanded(repo, true),
            Some(Selection::Agent { repo, agent }) => {
                self.set_agent_children_expanded(repo, agent, true);
            }
            Some(Selection::OtherHeader) => self.set_other_collapsed(false),
            Some(Selection::OrphanHeader) => self.set_orphans_collapsed(false),
            _ => {}
        }
    }

    pub(super) fn collapse_selected_tree_item(&mut self) {
        match self.selected_item() {
            Some(Selection::OverseerCategory(category)) => {
                // A nested category row (Ledger / Decisions under Details) has
                // no expansion of its own to collapse, so it folds its parent
                // away instead — the same shape as the inbox-item arm below.
                let target = category.parent().unwrap_or(category);
                self.set_overseer_category_expanded(target, false);
            }
            // An item row collapses the category that owns it, like a child row
            // folding away under its parent.
            Some(Selection::OverseerInbox(_)) => {
                self.set_overseer_category_expanded(crate::model::OverseerCategory::Inbox, false);
            }
            Some(Selection::Repo(repo)) => self.set_repo_expanded(repo, false),
            Some(Selection::Agent { repo, agent }) => {
                self.set_agent_children_expanded(repo, agent, false);
            }
            Some(Selection::OtherHeader) => self.set_other_collapsed(true),
            Some(Selection::OrphanHeader) => self.set_orphans_collapsed(true),
            _ => {}
        }
    }

    pub(super) fn toggle_selected_tree_header(&mut self, selection: Selection) -> bool {
        match selection {
            Selection::OverseerCategory(category) => self.toggle_overseer_category(category),
            Selection::OtherHeader => self.set_other_collapsed(!self.other_collapsed),
            Selection::OrphanHeader => self.set_orphans_collapsed(!self.orphans_collapsed),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, registry::Registry};

    #[test]
    fn the_control_ai_row_is_not_a_toggleable_header() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.overseer_visible = true;
        app.selected = 0;
        assert_eq!(app.selected_item(), Some(Selection::OverseerAi));

        // Unlike a category or section header, Enter on this row must fall
        // through to the attach action rather than being swallowed here —
        // the bug this row exists to fix (dropr:370).
        assert!(!app.toggle_selected_tree_header(Selection::OverseerAi));
    }

    #[test]
    fn h_and_l_do_not_touch_the_control_ai_row() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.overseer_visible = true;
        app.selected = 0;
        assert_eq!(app.selected_item(), Some(Selection::OverseerAi));

        app.expand_selected_tree_item();
        app.collapse_selected_tree_item();
        assert_eq!(app.selected_item(), Some(Selection::OverseerAi));
    }

    #[test]
    fn collapsing_a_details_child_folds_the_details_row_away() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.overseer_visible = true;
        app.set_overseer_category_expanded(crate::model::OverseerCategory::Details, true);
        app.selected = app
            .visible()
            .iter()
            .position(|row| {
                *row == Selection::OverseerCategory(crate::model::OverseerCategory::Ledger)
            })
            .expect("no Ledger child row");

        // A nested category row has no expansion of its own, so `h` folds its
        // parent away — the same shape as an inbox item collapsing Inbox.
        app.collapse_selected_tree_item();
        assert!(!app.overseer_category_expanded(crate::model::OverseerCategory::Details));
        assert!(!app.visible().contains(&Selection::OverseerCategory(
            crate::model::OverseerCategory::Ledger
        )));
    }

    #[test]
    fn a_discord_channel_row_is_not_a_toggleable_header() {
        // Same shape as `the_control_ai_row_is_not_a_toggleable_header`
        // (dropr:371): a channel row owns a session to attach to, not a
        // header to expand, so Enter must fall through to the attach action
        // rather than being swallowed here.
        assert!(
            !App::new(
                Registry::default(),
                Config::default(),
                tempfile::tempdir().unwrap().path().into(),
            )
            .toggle_selected_tree_header(Selection::DiscordChannel(0))
        );
    }
}
