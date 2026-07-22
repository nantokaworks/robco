use crate::model::Selection;

use super::super::App;

impl App {
    fn agent_children_key(&self, repo: usize, agent: usize) -> String {
        crate::ui::actions::discovery::path_key(
            &self.registry.repos[repo].agents[agent].worktree_path,
        )
    }

    pub(in crate::ui) fn agent_children_expanded(&self, repo: usize, agent: usize) -> bool {
        self.expanded_children
            .contains(&self.agent_children_key(repo, agent))
    }

    pub(in crate::ui) fn prune_expanded_children(&mut self) {
        let live = self
            .registry
            .repos
            .iter()
            .flat_map(|repo| &repo.agents)
            .map(|agent| crate::ui::actions::discovery::path_key(&agent.worktree_path))
            .collect::<std::collections::HashSet<_>>();
        self.expanded_children.retain(|key| live.contains(key));
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
        self.clamp_selection();
    }

    pub(super) fn expand_selected_tree_item(&mut self) {
        match self.selected_item() {
            Some(Selection::Overseer) => self.set_overseer_collapsed(false),
            Some(Selection::OverseerCategory(category)) => {
                self.set_overseer_category_expanded(category, true);
            }
            Some(Selection::Repo(repo)) => {
                if let Some(expanded) = self.expanded.get_mut(repo) {
                    *expanded = true;
                }
            }
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
            Some(Selection::Overseer) => self.set_overseer_collapsed(true),
            Some(Selection::OverseerCategory(category)) => {
                self.set_overseer_category_expanded(category, false);
            }
            Some(Selection::Repo(repo)) => {
                if let Some(expanded) = self.expanded.get_mut(repo) {
                    *expanded = false;
                }
            }
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
            Selection::Overseer => self.set_overseer_collapsed(!self.overseer_collapsed),
            Selection::OverseerCategory(category) => self.toggle_overseer_category(category),
            Selection::OtherHeader => self.set_other_collapsed(!self.other_collapsed),
            Selection::OrphanHeader => self.set_orphans_collapsed(!self.orphans_collapsed),
            _ => return false,
        }
        true
    }
}
