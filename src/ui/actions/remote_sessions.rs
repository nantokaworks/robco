//! Routes session reads and writes to the selected owning host when possible,
//! avoiding collisions when separate hosts use the same tmux session name.

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
        if let Some(repo) = self.selected_repo().filter(|repo| {
            let repo = &self.registry.repos[*repo];
            repo.host.is_some() && owns_session(self, repo, session)
        }) {
            return Some(repo);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Config, model::HostLabel, registry::Registry, ui::actions::remote_hosts::HostSlot,
    };

    #[test]
    fn duplicate_remote_session_names_prefer_selected_repo() {
        let repo = |ssh: &str| {
            let mut repo: RepoNode = serde_json::from_value(serde_json::json!({
                "path": format!("/{ssh}/shared"), "name": "shared",
                "remote_url": null, "pinned": true
            }))
            .unwrap();
            repo.host = Some(HostLabel {
                name: ssh.into(),
                ssh: ssh.into(),
            });
            repo
        };
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(
            Registry {
                version: 1,
                repos: vec![repo("first"), repo("selected")],
            },
            Config::default(),
            temp.path().into(),
        );
        app.hosts = ["first", "selected"]
            .into_iter()
            .map(|ssh| {
                HostSlot::idle(HostLabel {
                    name: ssh.into(),
                    ssh: ssh.into(),
                })
            })
            .collect();
        app.selected = app
            .visible()
            .iter()
            .position(|selection| matches!(selection, crate::model::Selection::Repo(1)))
            .unwrap();

        assert_eq!(app.remote_session_repo("robco_shared_main"), Some(1));
    }
}
