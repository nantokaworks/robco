use crate::{
    Result, agent,
    locale::{fmt, t},
    model::{HostLabel, Selection, Status},
    overseer, tmux,
};

use super::super::{App, suspend_terminal};

impl App {
    /// Suspend the TUI and hand the terminal to a tmux session. A failure here
    /// (e.g. the session exited between `ensure_*` and attach, or a transient
    /// tmux error) is surfaced as an in-app message and MUST NOT propagate: a
    /// bubbling `Err` would unwind out of the event loop and exit robco, which
    /// over ssh drops the whole connection.
    fn attach_session(&mut self, session: &str) {
        self.attach_session_on(session, None);
    }

    pub(in crate::ui) fn attach_session_on(&mut self, session: &str, host: Option<&HostLabel>) {
        self.force_redraw = true;
        let server = &self.config.tmux_server;
        let result = suspend_terminal(|| match host {
            Some(host) => remote_attach(host, session),
            None => tmux::attach(server, session),
        });
        if let Err(err) = result {
            self.show_message(err.to_string());
        }
    }

    pub(in crate::ui) fn attach_selected(&mut self) -> Result<()> {
        if let Some(Selection::ChildWorktree { repo, agent, child }) = self.selected_item() {
            let session = self.registry.repos[repo].agents[agent].children[child]
                .tmux_session
                .clone();
            if let Some(session) = session {
                let host = self.registry.repos[repo].host.clone();
                self.attach_session_on(&session, host.as_ref());
            } else {
                self.show_message(t(self.locale, "no live session in this child worktree"));
            }
            return Ok(());
        }
        let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        else {
            return Ok(());
        };
        let selected = self.registry.repos[repo].agents[agent_idx].clone();
        if selected.status == Status::BranchOnly {
            self.show_message(fmt(self.locale, "branch remains: {}", &[&selected.branch]));
            return Ok(());
        }
        let host = self.registry.repos[repo].host.clone();
        let ensured = if host.is_some() {
            Ok(())
        } else {
            agent::ensure_agent_session(&selected)
        };
        match ensured {
            Ok(()) => self.attach_session_on(&selected.tmux_session, host.as_ref()),
            Err(err) => self.show_message(err.to_string()),
        }
        Ok(())
    }

    pub(in crate::ui) fn attach_shell_selected(&mut self) -> Result<()> {
        match self.selected_item() {
            Some(Selection::Agent {
                repo,
                agent: agent_idx,
            }) => {
                let selected = self.registry.repos[repo].agents[agent_idx].clone();
                let host = self.registry.repos[repo].host.clone();
                if selected.status == Status::BranchOnly {
                    self.show_message(fmt(self.locale, "branch remains: {}", &[&selected.branch]));
                    return Ok(());
                }
                let ensured = if host.is_some() {
                    Ok(())
                } else {
                    agent::ensure_shell_session(&selected)
                };
                match ensured {
                    Ok(()) => {
                        self.attach_session_on(&agent::shell_session_name(&selected), host.as_ref())
                    }
                    Err(err) => self.show_message(err.to_string()),
                }
            }
            Some(Selection::Repo(repo)) => {
                let repo_node = self.registry.repos[repo].clone();
                let host = repo_node.host.clone();
                let prefix = self.config.tmux_session_prefix.clone();
                let ensured = if host.is_some() {
                    Ok(())
                } else {
                    agent::ensure_repo_shell_session(&prefix, &repo_node)
                };
                match ensured {
                    Ok(()) => self.attach_session_on(
                        &agent::repo_shell_session_name(&prefix, &repo_node),
                        host.as_ref(),
                    ),
                    Err(err) => self.show_message(err.to_string()),
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Attach the Overseer's own control session, creating it when absent.
    /// This is the row's Enter action (see `Selection::OverseerAi`); unlike
    /// `attach_claude_selected` it is not gated on the current preview tab,
    /// since the row's one tab is Info, not Claude.
    pub(in crate::ui) fn attach_control_selected(&mut self) {
        if !matches!(self.selected_item(), Some(Selection::OverseerAi)) {
            return;
        }
        let cwd = self
            .ephemeral_root
            .clone()
            .unwrap_or_else(|| self.config.repos_root.clone());
        match overseer::ensure_control_session(&self.config, &cwd) {
            Ok(session) => self.attach_session(&session),
            Err(err) => self.show_message(err.to_string()),
        }
    }

    pub(in crate::ui) fn attach_claude_selected(&mut self) -> Result<()> {
        match self.selected_item() {
            Some(Selection::Agent {
                repo,
                agent: agent_idx,
            }) => {
                let selected = self.registry.repos[repo].agents[agent_idx].clone();
                let host = self.registry.repos[repo].host.clone();
                if selected.status == Status::BranchOnly {
                    self.show_message(fmt(self.locale, "branch remains: {}", &[&selected.branch]));
                    return Ok(());
                }
                let ensured = if host.is_some() {
                    Ok(())
                } else {
                    agent::ensure_agent_session(&selected)
                };
                match ensured {
                    Ok(()) => self.attach_session_on(&selected.tmux_session, host.as_ref()),
                    Err(err) => self.show_message(err.to_string()),
                }
            }
            Some(Selection::Repo(repo)) => {
                let repo_node = self.registry.repos[repo].clone();
                let host = repo_node.host.clone();
                let prefix = self.config.tmux_session_prefix.clone();
                let ensured = if host.is_some() {
                    Ok(())
                } else {
                    agent::ensure_repo_claude_session(&self.config, &prefix, &repo_node)
                };
                match ensured {
                    Ok(()) => self.attach_session_on(
                        &agent::repo_claude_session_name(&prefix, &repo_node),
                        host.as_ref(),
                    ),
                    Err(err) => self.show_message(err.to_string()),
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(in crate::ui) fn instruct_overseer(&mut self, instruction: &str) {
        let cwd = self
            .ephemeral_root
            .clone()
            .unwrap_or_else(|| self.config.repos_root.clone());
        let result = overseer::ensure_control_session(&self.config, &cwd).and_then(|session| {
            tmux::send_literal_text(&self.config.tmux_server, &session, instruction)?;
            tmux::send_keys(&self.config.tmux_server, &session, &["Enter"])
        });
        match result {
            Ok(()) => self.show_message(t(self.locale, "instruction sent to overseer control")),
            Err(err) => self.show_message(err.to_string()),
        }
    }

    /// Send a one-line instruction into a repo/agent/orphan row's live
    /// CLAUDE/CODEX tmux session (dropr:565) — the session `Mode::PromptSession`
    /// was opened for. Mirrors `instruct_overseer`, but for the row-owned
    /// session rather than the control AI's.
    pub(in crate::ui) fn instruct_session(&mut self, session: &str, instruction: &str) {
        if self.is_remote_session(session) {
            let Some(client) = self.remote_client_for_session(session) else {
                self.show_message(t(self.locale, "remote host is not connected"));
                return;
            };
            match client.instruct_session(session, instruction) {
                Ok(outcome) if outcome.ok => {
                    self.show_message(t(self.locale, "instruction sent"));
                }
                Ok(_) => self.show_message(t(self.locale, "remote instruction was refused")),
                Err(error) => self.show_message(error.to_string()),
            }
            return;
        }
        let server = self.config.tmux_server.clone();
        self.instruct_session_with(
            session,
            instruction,
            |session, text| tmux::send_literal_text(&server, session, text),
            |session, keys| tmux::send_keys(&server, session, keys),
        );
    }

    fn instruct_session_with(
        &mut self,
        session: &str,
        instruction: &str,
        mut literal: impl FnMut(&str, &str) -> Result<()>,
        mut keys: impl FnMut(&str, &[&str]) -> Result<()>,
    ) {
        // A raw newline is itself a submit, so unflattened multi-line text
        // (e.g. a bracket-paste) would run as separate fragments instead of
        // one instruction (`tmux::single_line`'s own doc comment).
        let instruction = tmux::single_line(instruction);
        let result = literal(session, &instruction).and_then(|()| keys(session, &["Enter"]));
        match result {
            Ok(()) => self.show_message(t(self.locale, "instruction sent")),
            Err(err) => self.show_message(err.to_string()),
        }
    }

    /// Attach the selected Discord channel's tmux session (dropr:371). Unlike
    /// the control AI row there is nothing to create here: a channel session
    /// exists only while a turn is running and is torn down at the end of
    /// it, so an absent session means "no turn is running right now" rather
    /// than "not created yet" — and is said explicitly instead of silently
    /// doing nothing.
    pub(in crate::ui) fn attach_discord_channel_selected(&mut self, index: usize) {
        if !matches!(self.selected_item(), Some(Selection::DiscordChannel(_))) {
            return;
        }
        let ids =
            crate::ui::overseer::ordered_channel_ids(&self.overseer_snapshot.discord_channels);
        let Some(channel_id) = ids.get(index) else {
            self.show_message(t(self.locale, "channel is no longer listed"));
            return;
        };
        let session =
            overseer::discord_channel_session_name(&self.config.tmux_session_prefix, channel_id);
        match tmux::has_session(&self.config.tmux_server, &session) {
            Ok(true) => self.attach_session(&session),
            Ok(false) => {
                self.show_message(t(
                    self.locale,
                    "no live session — a turn is not running for this channel",
                ));
            }
            Err(err) => self.show_message(err.to_string()),
        }
    }

    pub(in crate::ui) fn attach_orphan_selected(&mut self) {
        let Some(Selection::Orphan(orphan)) = self.selected_item() else {
            return;
        };
        let Some(session) = self.orphans.get(orphan).map(|orphan| orphan.name.clone()) else {
            return;
        };
        self.attach_session(&session);
    }
}

/// Direct ssh attach intentionally nests the operator's local and remote tmux.
fn remote_attach(host: &HostLabel, session: &str) -> Result<()> {
    let target = format!("={session}");
    let status = std::process::Command::new("ssh")
        .args(["-t", &host.ssh, "tmux", "attach", "-t", &target])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(crate::Error::Command {
            context: "remote tmux attach",
            stderr: format!("ssh exited with {status}"),
        })
    }
}

#[cfg(test)]
#[path = "attach_tests.rs"]
mod tests;
