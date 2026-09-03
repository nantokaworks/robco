use std::path::Path;

use crate::{
    model::{OverseerCategory, RepoNode, Selection},
    overseer,
};

use super::{App, default_pane};
mod repo_rows;

impl App {
    pub(crate) fn effective_roots(&self) -> impl Iterator<Item = &std::path::Path> {
        std::iter::once(self.config.repos_root.as_path()).chain(
            self.ephemeral_root
                .as_deref()
                .filter(|root| *root != self.config.repos_root),
        )
    }

    pub(in crate::ui) fn item_key(&self, selection: Selection) -> String {
        match selection {
            Selection::OverseerAi => "overseer:control-ai".to_string(),
            Selection::OverseerAlert(item) => self.overseer_inbox.get(item).map_or_else(
                || "overseer-alert:missing".to_string(),
                |item| format!("overseer-alert:{}:{}", item.kind.code(), item.target_id),
            ),
            Selection::OverseerCategory(category) => {
                format!("overseer:{}", category.label().to_lowercase())
            }
            Selection::OverseerInbox(item) => self.overseer_inbox.get(item).map_or_else(
                || "overseer-inbox:missing".to_string(),
                |item| format!("overseer-inbox:{}:{}", item.kind.code(), item.target_id),
            ),
            Selection::DiscordChannel(index) => {
                super::overseer::ordered_channel_ids(&self.overseer_snapshot.discord_channels)
                    .get(index)
                    .map_or_else(
                        || "discord-channel:missing".to_string(),
                        |id| format!("discord-channel:{id}"),
                    )
            }
            Selection::RemoteControlAi(_) | Selection::RemoteDiscordChannel { .. } => {
                repo_rows::remote_item_key(self, selection)
            }
            Selection::Repo(repo) => format!(
                "repo:{}:{}",
                self.repo_host_key(repo),
                self.registry.repos[repo].path.display()
            ),
            Selection::RepoEscalation { repo, item } => self
                .registry
                .repos
                .get(repo)
                .zip(self.overseer_inbox.get(item))
                .map_or_else(
                    || "repo-escalation:missing".to_string(),
                    |(repo, item)| {
                        format!(
                            "repo-escalation:{}:{}:{}",
                            super::actions::discovery::path_key(&repo.path),
                            item.kind.code(),
                            item.target_id
                        )
                    },
                ),
            Selection::Agent { repo, agent } => format!(
                "agent:{}:{}",
                self.repo_host_key(repo),
                self.registry.repos[repo].agents[agent].id
            ),
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

    /// Restore the remembered valid preview tab, or the selection's default.
    pub(in crate::ui) fn restore_preview(&mut self) {
        let selection = self.selected_item();
        let panes = self.preview_panes(selection);
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
        if self.dropr_task_focus.is_some()
            && !matches!(self.selected_item(), Some(Selection::Repo(_)))
        {
            self.dropr_task_focus = None;
        }
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
        self.dropr_task_focus = None;
        let panes = self.preview_panes(Some(selection));
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
    /// Whether `repo` is a direct child of one of the effective discovery roots.
    pub(in crate::ui) fn repo_is_local(&self, repo: &RepoNode) -> bool {
        repo.path.parent().is_some_and(|parent| {
            self.effective_roots().into_iter().any(|root| {
                super::actions::discovery::path_key(parent)
                    == super::actions::discovery::path_key(root)
            })
        })
    }

    /// Registry indices of repos listed directly under a discovery root, in the
    /// order the operator arranged them.
    pub(in crate::ui) fn local_repos(&self) -> Vec<usize> {
        self.in_saved_order(
            self.registry
                .repos
                .iter()
                .enumerate()
                .filter(|(_, repo)| repo.host.is_none() && self.repo_is_local(repo))
                .map(|(idx, _)| idx)
                .collect(),
        )
    }

    /// Ordered off-launch-dir repos that still have agents or are pinned.
    pub(in crate::ui) fn other_location_repos(&self) -> Vec<usize> {
        self.in_saved_order(
            self.registry
                .repos
                .iter()
                .enumerate()
                .filter(|(_, repo)| {
                    repo.host.is_none()
                        && !self.repo_is_local(repo)
                        && (!repo.agents.is_empty() || repo.pinned)
                })
                .map(|(idx, _)| idx)
                .collect(),
        )
    }

    /// Flattened tree rows in display order.
    pub(in crate::ui) fn visible(&self) -> Vec<Selection> {
        let mut visible = Vec::new();
        if self.overseer_visible {
            visible.extend(
                self.global_escalations()
                    .into_iter()
                    .map(|(item, _)| Selection::OverseerAlert(item)),
            );
            visible.push(Selection::OverseerAi);
            for category in OverseerCategory::ALL {
                visible.push(Selection::OverseerCategory(category));
                if category.has_children() && self.overseer_category_expanded(category) {
                    match category {
                        OverseerCategory::Inbox => visible
                            .extend((0..self.overseer_inbox.len()).map(Selection::OverseerInbox)),
                        OverseerCategory::Discord => {
                            let count = super::overseer::ordered_channel_ids(
                                &self.overseer_snapshot.discord_channels,
                            )
                            .len();
                            visible.extend((0..count).map(Selection::DiscordChannel));
                        }
                        _ => {}
                    }
                }
            }
        }
        for repo_idx in self.local_repos() {
            repo_rows::push_repo_rows(self, &mut visible, repo_idx, &self.registry.repos[repo_idx]);
        }

        let others = self.other_location_repos();
        if !others.is_empty() {
            visible.push(Selection::OtherHeader);
            if !self.other_collapsed {
                for repo_idx in others {
                    repo_rows::push_repo_rows(
                        self,
                        &mut visible,
                        repo_idx,
                        &self.registry.repos[repo_idx],
                    );
                }
            }
        }
        for (host, slot) in self.hosts.iter().enumerate() {
            repo_rows::push_remote_host_rows(self, &mut visible, host, slot);
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

    pub(in crate::ui) fn set_overseer_visibility(&mut self, visible: bool) {
        if self.overseer_visible == visible {
            return;
        }
        let selected_identity = self.selected_item().map(|sel| self.item_key(sel));
        self.overseer_visible = visible;
        self.restore_selection(selected_identity);
    }
}

pub(super) fn overseer_is_visible() -> bool {
    overseer::pidfile_path()
        .ok()
        .zip(overseer::ledger_path().ok())
        .is_some_and(|(pidfile, ledger)| overseer_artifacts_exist(&pidfile, &ledger))
}

fn overseer_artifacts_exist(pidfile: &Path, ledger: &Path) -> bool {
    pidfile.is_file() && ledger.is_file()
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;
