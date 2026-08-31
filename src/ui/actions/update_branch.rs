//! `u` on an agent row: bring its pull request's branch up to date with its
//! base, the same action the `robco_pr_update_branch` MCP tool runs
//! (`crate::pr_update`).
//!
//! Unlike merge, this never touches the worktree or the primary checkout —
//! `gh pr update-branch` runs entirely on GitHub's own side — so it is safe
//! to run synchronously on the UI thread, the same way `checkout_main`'s and
//! `merge_selected`'s own pre-merge reads already do.

use crate::{
    locale::{fmt, t},
    model::Selection,
    pr_update::{self, UpdateOutcome},
};

use super::super::App;

/// The one refusal this action decides purely from the selection kind,
/// before it ever touches the registry — pulled out so it is testable
/// without building a full row to select (mirrors
/// `actions::pr::pr_target_for_selection`'s own `ChildWorktree` case).
fn selection_refusal(selection: Option<Selection>) -> Option<&'static str> {
    matches!(selection, Some(Selection::ChildWorktree { .. }))
        .then_some("branch update is not available for child worktrees")
}

impl App {
    pub(in crate::ui) fn update_branch_selected(&mut self) {
        if let Some(message) = selection_refusal(self.selected_item()) {
            self.show_message(t(self.locale, message));
            return;
        }
        let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        else {
            return;
        };
        let repo_node = &self.registry.repos[repo];
        let agent = &repo_node.agents[agent_idx];
        let repo_path = repo_node.path.clone();
        let branch = agent.branch.clone();
        let agent_id = agent.id.clone();
        let strategy = self.config.merge_strategy;

        match pr_update::update_behind(&repo_path, &branch, &agent_id, strategy, "ui") {
            Ok(UpdateOutcome::Updated) => {
                self.show_message(fmt(self.locale, "updated {} from its base", &[&branch]));
            }
            Ok(UpdateOutcome::AlreadyUpToDate) => {
                self.show_message(fmt(
                    self.locale,
                    "{} is already up to date with its base",
                    &[&branch],
                ));
            }
            Err(err) => self.show_message(err.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "update_branch_tests.rs"]
mod tests;
