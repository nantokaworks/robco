use std::path::{Path, PathBuf};

use crate::{
    locale::{Locale, fmt, t},
    model::Selection,
    rename::{self, RenameOutcome},
};

use super::super::{App, Mode, text_input::TextInput};

/// One line per worktree `git worktree repair` could not fix, each with the
/// exact command to fix it by hand — the same one robco just ran itself.
fn unrepaired_lines(locale: Locale, new_path: &Path, outcome: &RenameOutcome) -> Vec<String> {
    let mut lines = vec![fmt(
        locale,
        "renamed to {}",
        &[&new_path.display().to_string()],
    )];
    lines.push(t(locale, "these worktrees still need manual repair:").to_string());
    for (worktree, error) in &outcome.unrepaired_worktrees {
        lines.push(format!("  {}: {error}", worktree.display()));
    }
    lines.push(fmt(
        locale,
        "run: git -C {} worktree repair <worktree-path>",
        &[&new_path.display().to_string()],
    ));
    lines
}

impl App {
    /// `g` on a selected repository row: opens the rename prompt, or explains
    /// why not. Mirrors `confirm_kill_selected`'s repo-row precondition style
    /// (agents must be gone first) but has no `pinned` gate — see the rename
    /// decision scribble on dropr:505 for why the two actions differ there.
    pub(in crate::ui) fn open_rename_prompt(&mut self) {
        let Some(Selection::Repo(repo)) = self.selected_item() else {
            return;
        };
        let repo_node = &self.registry.repos[repo];
        if !repo_node.agents.is_empty() {
            self.show_message(t(self.locale, "remove agents first"));
            return;
        }
        self.mode = Mode::PromptRenameRepo {
            path: repo_node.path.clone(),
            input: TextInput::from(repo_node.name.as_str()),
        };
    }

    pub(in crate::ui) fn rename_repo(&mut self, path: &Path, new_name: &str) {
        let Some(repo) = self
            .registry
            .repos
            .iter()
            .position(|repo| repo.path == path)
        else {
            self.show_message(t(self.locale, "repository changed, not renamed"));
            return;
        };
        if !self.registry.repos[repo].agents.is_empty() {
            self.show_message(t(self.locale, "repository changed, not renamed"));
            return;
        }
        if new_name == self.registry.repos[repo].name {
            return;
        }

        let old_path = path.to_path_buf();
        if self.registry.repos[repo].host.is_some() {
            let Some(client) = self.remote_client_for_repo(repo) else {
                self.show_message(t(self.locale, "remote host is not connected"));
                return;
            };
            match client.rename_repo(&old_path.display().to_string(), new_name) {
                Ok(outcome) if outcome.ok => {
                    let node = &mut self.registry.repos[repo];
                    node.path = PathBuf::from(&outcome.new_path);
                    node.name = new_name.to_string();
                    if outcome.unrepaired_worktrees.is_empty() {
                        self.show_message(fmt(self.locale, "renamed to {}", &[new_name]));
                    } else {
                        let details = outcome
                            .unrepaired_worktrees
                            .iter()
                            .map(|item| format!("  {}: {}", item.path, item.error))
                            .collect();
                        self.mode = Mode::ErrorDialog {
                            title: t(self.locale, "rename incomplete").into(),
                            lines: details,
                            force_kill: None,
                        };
                    }
                }
                Ok(_) => self.show_message(t(self.locale, "remote rename was refused")),
                Err(error) => self.show_message(error.to_string()),
            }
            return;
        }
        match rename::rename_repo_dir(&old_path, new_name) {
            Ok(outcome) => self.finish_rename(old_path, outcome, new_name),
            Err(error) => self.show_message(error.to_string()),
        }
    }

    fn finish_rename(&mut self, old_path: PathBuf, outcome: RenameOutcome, new_name: &str) {
        let new_path = outcome.new_path.clone();
        let new_name = new_name.to_string();
        let mut applied = false;
        let update_result = self.locked_registry_update(|registry| {
            applied = rename::apply_rename(registry, &old_path, &new_path, &new_name);
        });

        if let Err(error) = update_result {
            // The directory really did move; say where it went even though
            // the registry could not be told, since robco's own state may
            // now disagree with the filesystem.
            self.show_message(fmt(
                self.locale,
                "renamed the directory to {}, but could not update robco's saved state ({}); restart robco to re-discover it",
                &[&new_path.display().to_string(), &error.to_string()],
            ));
            return;
        }
        if !applied {
            self.show_message(fmt(
                self.locale,
                "renamed the directory to {}, but it was no longer in robco's registry",
                &[&new_path.display().to_string()],
            ));
            return;
        }

        self.clamp_selection();
        if outcome.unrepaired_worktrees.is_empty() {
            self.show_message(fmt(self.locale, "renamed to {}", &[&new_name]));
        } else {
            self.mode = Mode::ErrorDialog {
                title: t(self.locale, "rename incomplete").into(),
                lines: unrepaired_lines(self.locale, &new_path, &outcome),
                force_kill: None,
            };
        }
    }
}

#[cfg(test)]
#[path = "rename_tests.rs"]
mod tests;
