use std::collections::HashSet;

use crate::{agent, model::OrphanSession, overseer, tmux};

use super::super::App;

impl App {
    /// Rebuild the orphan-session list: robco-prefixed tmux sessions that no
    /// tracked agent (or its `-shell` twin) and no registry repo's derived
    /// main / main-shell session accounts for. These are sessions robco would
    /// otherwise never show again — e.g. left behind by a pre-#66 registry
    /// wipe or a deleted worktree. Runs on the discovery tick; a tmux failure
    /// keeps the previous list rather than flickering the section away.
    pub(in crate::ui) fn refresh_orphans(&mut self) {
        let Some(orphans) = discover_orphans(
            &self.registry.repos,
            &self.config.tmux_session_prefix,
            &self.config.worktree_root,
        ) else {
            return;
        };

        if orphans.len() != self.orphans.len()
            || orphans
                .iter()
                .zip(&self.orphans)
                .any(|(a, b)| a.name != b.name)
        {
            self.orphans = orphans;
            self.clamp_selection();
        }
    }

    /// Kill an orphan session by NAME (not index): the orphan list is rebuilt
    /// on every discovery tick, so this always targets exactly the session the
    /// confirm dialog displayed.
    pub(in crate::ui) fn kill_orphan(&mut self, session: &str) {
        match tmux::kill_session(session) {
            Ok(()) => {
                self.orphans.retain(|orphan| orphan.name != session);
                self.show_message(format!("killed {session}"));
                self.clamp_selection();
            }
            Err(err) => self.show_message(err.to_string()),
        }
    }
}

pub(super) fn discover_orphans(
    repos: &[crate::model::RepoNode],
    prefix: &str,
    worktree_root: &std::path::Path,
) -> Option<Vec<OrphanSession>> {
    let sessions = tmux::list_sessions_with_cwd().ok()?;
    let mut known: HashSet<String> = HashSet::new();
    known.insert(overseer::control_session_name(prefix));
    for repo in repos {
        known.insert(agent::repo_claude_session_name(prefix, repo));
        known.insert(agent::repo_shell_session_name(prefix, repo));
        for tracked in &repo.agents {
            known.insert(tracked.tmux_session.clone());
            known.insert(agent::shell_session_name(tracked));
            known.extend(
                tracked
                    .children
                    .iter()
                    .filter_map(|child| child.tmux_session.clone()),
            );
        }
    }
    let mut orphans = sessions
        .into_iter()
        .filter(|(name, cwd)| {
            name.starts_with(prefix)
                && !known.contains(name)
                && super::discovery::is_managed_worktree(cwd, worktree_root)
        })
        .map(|(name, cwd)| OrphanSession { name, cwd })
        .collect::<Vec<_>>();
    orphans.sort_by(|a, b| a.name.cmp(&b.name));
    Some(orphans)
}
