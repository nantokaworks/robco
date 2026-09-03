//! Routes session reads and writes to the selected owning host when possible,
//! avoiding collisions when separate hosts use the same tmux session name.

use ratatui::text::Text;

use crate::{
    locale::t,
    model::{HostLabel, RepoNode, Selection},
    remote::RemoteClient,
};

use crate::ui::{App, backend::Backend};

impl App {
    pub(in crate::ui) fn instruct_prompt_session(
        &mut self,
        host: Option<&HostLabel>,
        session: &str,
        instruction: &str,
    ) {
        match host {
            Some(host) => self.instruct_remote_session(host, session, instruction),
            None => self.instruct_session(session, instruction),
        }
    }

    pub(in crate::ui) fn instruct_remote_session(
        &mut self,
        host: &HostLabel,
        session: &str,
        instruction: &str,
    ) {
        let Some(client) = self
            .hosts
            .iter()
            .find(|slot| slot.label == *host)
            .and_then(|slot| slot.backend())
            .map(|backend| backend.client())
        else {
            self.show_message(t(self.locale, "remote host is not connected"));
            return;
        };
        match client.instruct_session(session, instruction) {
            Ok(outcome) if outcome.ok => self.show_message(t(self.locale, "instruction sent")),
            Ok(_) => self.show_message(t(self.locale, "remote instruction was refused")),
            Err(error) => self.show_message(error.to_string()),
        }
    }

    pub(in crate::ui) fn remote_client_for_session(&self, session: &str) -> Option<RemoteClient> {
        self.hosts
            .get(self.remote_session_host(session)?)?
            .backend()
            .map(|backend| backend.client())
    }

    pub(in crate::ui) fn is_remote_session(&self, session: &str) -> bool {
        self.remote_session_host(session).is_some()
    }

    pub(in crate::ui) fn remote_cached_tmux(&self, session: &str) -> Option<Text<'static>> {
        self.hosts
            .get(self.remote_session_host(session)?)?
            .backend()?
            .cached_tmux(&self.preview_capture, session)
    }

    pub(in crate::ui) fn remote_host_for_selection(&self) -> Option<usize> {
        match self.selected_item()? {
            Selection::RemoteControlAi(host) | Selection::RemoteDiscordChannel { host, .. } => {
                Some(host)
            }
            _ => {
                let repo = self.selected_repo()?;
                let label = self.registry.repos.get(repo)?.host.as_ref()?;
                self.hosts.iter().position(|slot| slot.label == *label)
            }
        }
    }

    fn remote_session_host(&self, session: &str) -> Option<usize> {
        if self.selected_local_global_session(session) {
            return None;
        }
        if let Some(host) = self
            .remote_host_for_selection()
            .filter(|host| owns_host_session(self, *host, session))
        {
            return Some(host);
        }
        if let Some(repo) = self.remote_session_repo(session) {
            let label = self.registry.repos[repo].host.as_ref()?;
            return self.hosts.iter().position(|slot| slot.label == *label);
        }
        (0..self.hosts.len()).find(|host| owns_host_session(self, *host, session))
    }

    fn selected_local_global_session(&self, session: &str) -> bool {
        let prefix = &self.config.tmux_session_prefix;
        match self.selected_item() {
            Some(Selection::OverseerAi) => crate::overseer::control_session_name(prefix) == session,
            Some(Selection::DiscordChannel(channel)) => {
                crate::ui::overseer::ordered_channel_ids(&self.overseer_snapshot.discord_channels)
                    .get(channel)
                    .is_some_and(|id| {
                        crate::overseer::discord_channel_session_name(prefix, id) == session
                    })
            }
            _ => false,
        }
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

fn owns_host_session(app: &App, host: usize, session: &str) -> bool {
    let prefix = &app.config.tmux_session_prefix;
    if crate::overseer::control_session_name(prefix) == session {
        return true;
    }
    let Some(view) = app.host_view(host) else {
        return false;
    };
    crate::ui::overseer::ordered_channel_ids(&view.discord_channels)
        .iter()
        .any(|id| crate::overseer::discord_channel_session_name(prefix, id) == session)
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
        app.sync_remote_host_views();
        app.selected = app
            .visible()
            .iter()
            .position(|selection| matches!(selection, crate::model::Selection::Repo(1)))
            .unwrap();

        assert_eq!(app.remote_session_repo("robco_shared_main"), Some(1));
    }

    #[test]
    fn duplicate_control_session_prefers_selected_host() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.hosts = ["first", "selected"]
            .into_iter()
            .map(|ssh| {
                HostSlot::connected(HostLabel {
                    name: ssh.into(),
                    ssh: ssh.into(),
                })
            })
            .collect();
        app.sync_remote_host_views();
        app.selected = app
            .visible()
            .iter()
            .position(|selection| *selection == crate::model::Selection::RemoteControlAi(1))
            .unwrap();

        let session = crate::overseer::control_session_name(&app.config.tmux_session_prefix);
        assert_eq!(app.remote_session_host(&session), Some(1));
    }

    #[test]
    fn selected_local_control_session_stays_local() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.hosts = vec![HostSlot::connected(HostLabel {
            name: "remote".into(),
            ssh: "remote".into(),
        })];
        app.set_overseer_visibility(true);
        app.selected = 0;

        let session = crate::overseer::control_session_name(&app.config.tmux_session_prefix);
        assert_eq!(app.selected_item(), Some(Selection::OverseerAi));
        assert_eq!(app.remote_session_host(&session), None);
    }
}
