use std::path::Path;

use crate::{
    chief,
    model::{RepoNode, Selection},
};

use super::{App, default_pane, panes_for};

impl App {
    /// Stable identity for the current selection, used to remember its preview
    /// tab. Indices shift as items are added or removed, so repos key on their
    /// path and agents on their unique id.
    pub(in crate::ui) fn item_key(&self, selection: Selection) -> String {
        match selection {
            Selection::Chief => "chief".to_string(),
            Selection::Repo(repo) => {
                format!("repo:{}", self.registry.repos[repo].path.display())
            }
            Selection::Agent { repo, agent } => {
                format!("agent:{}", self.registry.repos[repo].agents[agent].id)
            }
            Selection::ChildWorktree { repo, agent, child } => format!(
                "child:{}",
                self.registry.repos[repo].agents[agent].children[child]
                    .path
                    .display()
            ),
            Selection::OtherHeader => "section:other".to_string(),
            Selection::OrphanHeader => "section:orphans".to_string(),
            Selection::Orphan(orphan) => format!(
                "orphan:{}",
                self.orphans
                    .get(orphan)
                    .map(|orphan| orphan.name.as_str())
                    .unwrap_or_default()
            ),
        }
    }

    /// Set the active preview pane from the remembered tab for the current
    /// selection, falling back to that selection's default tab. Guards against a
    /// stale pane that is not valid for the current selection type.
    pub(in crate::ui) fn restore_preview(&mut self) {
        let selection = self.selected_item();
        let panes = panes_for(selection);
        let remembered = selection
            .map(|sel| self.item_key(sel))
            .and_then(|key| self.preview_tabs.get(&key).copied())
            .filter(|pane| panes.contains(pane));
        self.preview = remembered.unwrap_or_else(|| default_pane(selection));
    }

    pub(in crate::ui) fn selected_item(&self) -> Option<Selection> {
        self.visible().get(self.selected).copied()
    }

    pub(in crate::ui) fn clamp_selection(&mut self) {
        let len = self.visible().len();
        let previous = self.selected;
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
        if self.selected != previous {
            self.restore_preview();
        }
    }

    pub(in crate::ui) fn move_selection_down(&mut self) {
        let len = self.visible().len();
        if self.selected + 1 < len {
            self.selected += 1;
            self.preview_scroll = 0;
            self.restore_preview();
        }
    }

    pub(in crate::ui) fn move_selection_up(&mut self) {
        let previous = self.selected;
        self.selected = self.selected.saturating_sub(1);
        if self.selected != previous {
            self.preview_scroll = 0;
            self.restore_preview();
        }
    }

    pub(in crate::ui) fn selected_repo(&self) -> Option<usize> {
        match self.selected_item() {
            Some(Selection::Repo(repo)) => Some(repo),
            Some(Selection::Agent { repo, .. }) => Some(repo),
            Some(Selection::ChildWorktree { repo, .. }) => Some(repo),
            _ => None,
        }
    }

    pub(in crate::ui) fn toggle_preview(&mut self) {
        self.cycle_preview(1);
    }

    pub(in crate::ui) fn toggle_preview_back(&mut self) {
        self.cycle_preview(-1);
    }

    fn cycle_preview(&mut self, step: isize) {
        let Some(selection) = self.selected_item() else {
            return;
        };
        let panes = panes_for(Some(selection));
        if panes.is_empty() {
            return;
        }
        let current = panes.iter().position(|pane| *pane == self.preview);
        let next = panes[current.map_or(0, |idx| {
            (idx as isize + step).rem_euclid(panes.len() as isize) as usize
        })];
        self.preview = next;
        self.preview_scroll = 0;
        let key = self.item_key(selection);
        self.preview_tabs.insert(key, next);
    }
    /// Whether `repo` was discovered under the current launch directory (a
    /// direct child), as opposed to carried over from a launch elsewhere.
    pub(in crate::ui) fn repo_is_local(&self, repo: &RepoNode) -> bool {
        repo.path.parent() == Some(self.launch_dir.as_path())
    }

    /// Registry indices of off-launch-dir repos that still have agents or were
    /// pinned by manual registration.
    pub(in crate::ui) fn other_location_repos(&self) -> Vec<usize> {
        self.registry
            .repos
            .iter()
            .enumerate()
            .filter(|(_, repo)| {
                !self.repo_is_local(repo) && (!repo.agents.is_empty() || repo.pinned)
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    /// Flattened tree rows in display order: local repos first, then — when any
    /// off-launch-dir repo still has agents — the collapsible "other locations"
    /// section listing them.
    pub(in crate::ui) fn visible(&self) -> Vec<Selection> {
        let mut visible = Vec::new();
        if self.chief_visible {
            visible.push(Selection::Chief);
        }
        for (repo_idx, repo) in self.registry.repos.iter().enumerate() {
            if self.repo_is_local(repo) {
                self.push_repo_rows(&mut visible, repo_idx, repo);
            }
        }

        let others = self.other_location_repos();
        if !others.is_empty() {
            visible.push(Selection::OtherHeader);
            if !self.other_collapsed {
                for repo_idx in others {
                    self.push_repo_rows(&mut visible, repo_idx, &self.registry.repos[repo_idx]);
                }
            }
        }

        if !self.orphans.is_empty() {
            visible.push(Selection::OrphanHeader);
            if !self.orphans_collapsed {
                for orphan_idx in 0..self.orphans.len() {
                    visible.push(Selection::Orphan(orphan_idx));
                }
            }
        }
        visible
    }

    fn push_repo_rows(&self, visible: &mut Vec<Selection>, repo_idx: usize, repo: &RepoNode) {
        visible.push(Selection::Repo(repo_idx));
        if self.expanded.get(repo_idx).copied().unwrap_or(true) {
            for (agent_idx, _) in crate::model::agent_order(&repo.agents) {
                visible.push(Selection::Agent {
                    repo: repo_idx,
                    agent: agent_idx,
                });
                for child in 0..repo.agents[agent_idx].children.len() {
                    visible.push(Selection::ChildWorktree {
                        repo: repo_idx,
                        agent: agent_idx,
                        child,
                    });
                }
            }
        }
    }

    pub(in crate::ui) fn set_other_collapsed(&mut self, collapsed: bool) {
        self.other_collapsed = collapsed;
        self.clamp_selection();
    }

    pub(in crate::ui) fn set_orphans_collapsed(&mut self, collapsed: bool) {
        self.orphans_collapsed = collapsed;
        self.clamp_selection();
    }

    pub(in crate::ui) fn refresh_chief_visibility(&mut self) {
        self.set_chief_visibility(chief_is_visible());
    }

    fn set_chief_visibility(&mut self, visible: bool) {
        if self.chief_visible == visible {
            return;
        }
        let selected_identity = self.selected_item().map(|sel| self.item_key(sel));
        self.chief_visible = visible;
        self.restore_selection(selected_identity);
    }
}

pub(super) fn chief_is_visible() -> bool {
    chief::pidfile_path()
        .ok()
        .zip(chief::ledger_path().ok())
        .is_some_and(|(pidfile, ledger)| chief_artifacts_exist(&pidfile, &ledger))
}

fn chief_artifacts_exist(pidfile: &Path, ledger: &Path) -> bool {
    pidfile.is_file() && ledger.is_file()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::{config::Config, registry::Registry};

    #[test]
    fn chief_visibility_requires_both_daemon_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let pidfile = temp.path().join("chief.pid");
        let ledger = temp.path().join("ledger.json");
        assert!(!chief_artifacts_exist(&pidfile, &ledger));
        fs::write(&pidfile, "123").unwrap();
        assert!(!chief_artifacts_exist(&pidfile, &ledger));
        fs::write(&ledger, "{}").unwrap();
        assert!(chief_artifacts_exist(&pidfile, &ledger));
        fs::remove_file(&pidfile).unwrap();
        assert!(!chief_artifacts_exist(&pidfile, &ledger));
    }

    #[test]
    fn selection_identity_survives_chief_row_toggle() {
        let temp = tempfile::tempdir().unwrap();
        let repo_path = temp.path().join("repo");
        let repo = serde_json::from_value(serde_json::json!({
            "path": repo_path,
            "name": "repo",
            "remote_url": null,
            "agents": []
        }))
        .unwrap();
        let registry = Registry {
            version: 1,
            repos: vec![repo],
        };
        let mut app = App::new(registry, Config::default(), temp.path().into());
        app.set_chief_visibility(false);
        assert!(matches!(app.selected_item(), Some(Selection::Repo(0))));

        app.set_chief_visibility(true);
        assert_eq!(app.selected, 1);
        assert!(matches!(app.selected_item(), Some(Selection::Repo(0))));

        app.set_chief_visibility(false);
        assert_eq!(app.selected, 0);
        assert!(matches!(app.selected_item(), Some(Selection::Repo(0))));
    }
}
