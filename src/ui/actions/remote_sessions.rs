//! Routes session reads and writes to the host that owns the selected row.

use ratatui::text::Text;

use crate::{model::RepoNode, remote::RemoteClient};

use crate::ui::{App, backend::Backend};

impl App {
    pub(in crate::ui) fn remote_client_for_session(&self, session: &str) -> Option<RemoteClient> {
        self.remote_client_for_repo(self.remote_session_repo(session)?)
    }

    pub(in crate::ui) fn is_remote_session(&self, session: &str) -> bool {
        self.remote_session_repo(session).is_some()
    }

    pub(in crate::ui) fn remote_cached_tmux(&self, session: &str) -> Option<Text<'static>> {
        let repo = self.remote_session_repo(session)?;
        let host = self.registry.repos[repo].host.as_ref()?;
        self.hosts
            .iter()
            .find(|slot| slot.label == *host)?
            .backend()?
            .cached_tmux(&self.preview_capture, session)
    }

    fn remote_session_repo(&self, session: &str) -> Option<usize> {
        self.registry
            .repos
            .iter()
            .position(|repo| repo.host.is_some() && owns_session(self, repo, session))
    }
}

fn owns_session(app: &App, repo: &RepoNode, session: &str) -> bool {
    repo.agents.iter().any(|agent| {
        agent.tmux_session == session
            || agent
                .children
                .iter()
                .any(|child| child.tmux_session.as_deref() == Some(session))
    }) || crate::agent::repo_claude_session_name(&app.config.tmux_session_prefix, repo) == session
        || crate::agent::repo_shell_session_name(&app.config.tmux_session_prefix, repo) == session
}
