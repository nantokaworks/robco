//! `c` on a repo row: return the primary checkout to `main`.
//!
//! The one place in the TUI allowed to move the primary checkout's `HEAD` —
//! `git::post_merge` and `overseer::release_pipeline` deliberately never do,
//! see their module docs — so this stays reachable only from a deliberate
//! keypress, never from the daemon or any background refresh.

use crate::{git, locale::t, model::Selection, status};

use super::super::App;

impl App {
    pub(in crate::ui) fn checkout_main_selected(&mut self) {
        let Some(Selection::Repo(repo)) = self.selected_item() else {
            self.show_message(t(
                self.locale,
                "c: select a repo to check out main in its primary checkout",
            ));
            return;
        };
        let repo_path = self.registry.repos[repo].path.clone();
        match git::worktree_is_clean(&repo_path) {
            Ok(true) => {}
            Ok(false) => {
                self.show_message(t(
                    self.locale,
                    "commit or clean untracked changes before checking out main",
                ));
                return;
            }
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        }
        if let Err(err) = git::checkout(&repo_path, "main") {
            self.show_message(err.to_string());
            return;
        }
        status::refresh_checkout_branch(&mut self.registry.repos[repo]);
        self.show_message(t(self.locale, "checked out main"));
    }
}

#[cfg(test)]
#[path = "checkout_main_tests.rs"]
mod tests;
