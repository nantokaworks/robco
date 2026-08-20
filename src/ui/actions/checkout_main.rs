//! `c` on a repo row: return the primary checkout to its own default branch.
//!
//! The one place in the TUI allowed to move the primary checkout's `HEAD` —
//! `git::post_merge` and `overseer::release_pipeline` deliberately never do,
//! see their module docs — so this stays reachable only from a deliberate
//! keypress, never from the daemon or any background refresh.

use crate::{
    git,
    locale::{fmt, t},
    model::Selection,
    status,
};

use super::super::App;

impl App {
    pub(in crate::ui) fn checkout_main_selected(&mut self) {
        let Some(Selection::Repo(repo)) = self.selected_item() else {
            self.show_message(t(
                self.locale,
                "c: select a repo to check out its default branch in its primary checkout",
            ));
            return;
        };
        let repo_path = self.registry.repos[repo].path.clone();
        // Resolved fresh, never assumed: see dropr:503. A repository whose
        // `origin/HEAD` cannot be read has no default branch this action can
        // safely check out.
        let default_branch = match git::default_branch(&repo_path) {
            Ok(Some(branch)) => branch,
            Ok(None) => {
                self.show_message(t(
                    self.locale,
                    "default branch could not be resolved — run git remote set-head origin -a",
                ));
                return;
            }
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        };
        match git::worktree_is_clean(&repo_path) {
            Ok(true) => {}
            Ok(false) => {
                self.show_message(fmt(
                    self.locale,
                    "commit or clean untracked changes before checking out {}",
                    &[&default_branch],
                ));
                return;
            }
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        }
        if let Err(err) = git::checkout(&repo_path, &default_branch) {
            self.show_message(err.to_string());
            return;
        }
        status::refresh_checkout_branch(&mut self.registry.repos[repo]);
        self.show_message(fmt(self.locale, "checked out {}", &[&default_branch]));
    }
}

#[cfg(test)]
#[path = "checkout_main_tests.rs"]
mod tests;
