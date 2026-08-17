//! Per-channel session slots for the conversational ops agent.
//!
//! `OpsAgent` used to hold a single `active: Option<Active>` slot, correct
//! only while conversation was confined to one channel. Chat scoped to a
//! whole Discord category (see `category.rs`) means two people in two
//! different channels can talk to the agent at once, so the slot became a
//! per-channel map — each session is still a spawned OS thread running an
//! agent CLI, so a `cap` on concurrent sessions is what keeps a chatty
//! category from exhausting the machine; overflow gets the same busy
//! message a single-slot agent already returned.

use super::ops_agent::{PendingSession, SessionRequest, SessionSpawner};
use super::ops_effect::{Effect, react_effect};
use super::reactions::ReactionStage;
use crate::{config::Config, overseer::discord_channels::DiscordChannels};
use std::{collections::HashMap, path::Path};

struct Active {
    request: SessionRequest,
    session: Box<dyn PendingSession>,
}

pub(super) enum StartOutcome {
    Started,
    Busy,
    AtCapacity,
    SpawnFailed(String),
}

pub(super) struct SessionSlots {
    active: HashMap<String, Active>,
    cap: usize,
}

impl SessionSlots {
    pub fn new(cap: usize) -> Self {
        Self {
            active: HashMap::new(),
            cap: cap.max(1),
        }
    }

    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
    }

    pub fn start(
        &mut self,
        channel_id: &str,
        request: SessionRequest,
        spawner: &mut dyn SessionSpawner,
    ) -> StartOutcome {
        if self.active.contains_key(channel_id) {
            return StartOutcome::Busy;
        }
        if self.active.len() >= self.cap {
            return StartOutcome::AtCapacity;
        }
        match spawner.spawn(request.clone()) {
            Ok(session) => {
                self.active
                    .insert(channel_id.to_string(), Active { request, session });
                StartOutcome::Started
            }
            Err(error) => StartOutcome::SpawnFailed(error),
        }
    }

    /// Channel ids with a session still outstanding, for the gateway's
    /// typing-indicator keepalive to reconcile against.
    pub fn active_channels(&self) -> impl Iterator<Item = &str> {
        self.active.keys().map(String::as_str)
    }

    /// Polls every active session, draining and reporting each that finished.
    /// `channels` records each finished turn's outcome (dropr:363) so the
    /// info pane and the next briefing's history both stay current.
    pub fn poll(&mut self, channels: &mut DiscordChannels, channels_path: &Path) -> Vec<Effect> {
        let mut finished = Vec::new();
        self.active.retain(|_, active| match active.session.poll() {
            Some(result) => {
                finished.push((active.request.clone(), result));
                false
            }
            None => true,
        });
        finished
            .into_iter()
            .flat_map(|(request, result)| {
                effects_from_result(&request, result, channels, channels_path)
            })
            .collect()
    }
}

/// The repository `channel_id` is bound to, read fresh from config so a
/// binding edited mid-run takes effect on the next completed turn without a
/// daemon restart — the same freshness `SystemSessionSpawner::spawn` already
/// gives the briefing itself. A config load failure degrades to unbound
/// rather than failing the whole turn over a cosmetic header.
fn bound_repo(channel_id: &str) -> Option<String> {
    Config::load()
        .ok()?
        .overseer
        .discord
        .channel_repo_bindings
        .get(channel_id)
        .cloned()
}

/// Prefixes a bound channel's reply with the repo it answered for (dropr:450),
/// so the operator can tell which repo answered without re-reading the
/// channel's binding. Unbound channels — the default — get the reply
/// unchanged, exactly as before.
fn with_repo_header(bound_repo: Option<&str>, reply: String) -> String {
    match bound_repo {
        Some(repo) => format!("**[{repo}]**\n{reply}"),
        None => reply,
    }
}

fn record_turn(
    channels: &mut DiscordChannels,
    path: &Path,
    request: &SessionRequest,
    outcome: Result<&str, &str>,
) {
    if let Err(error) = channels.end_turn(path, &request.channel_id, &request.message, outcome) {
        eprintln!("overseer: failed to record Discord channel state: {error}");
    }
}

fn effects_from_result(
    request: &SessionRequest,
    result: Result<Vec<u8>, String>,
    channels: &mut DiscordChannels,
    channels_path: &Path,
) -> Vec<Effect> {
    let terminal = |stage| react_effect(&request.channel_id, &request.message_id, stage);
    let raw = match result {
        Ok(raw) => raw,
        Err(error) => {
            record_turn(channels, channels_path, request, Err(&error));
            return vec![
                terminal(ReactionStage::Failure),
                Effect::Post {
                    channel_id: request.channel_id.clone(),
                    text: format!("Ops agent failed: {error}"),
                },
            ];
        }
    };
    let parsed = match super::ops_result::parse(&raw) {
        Ok(parsed) => parsed,
        Err(error) => {
            record_turn(channels, channels_path, request, Err(&error));
            return vec![
                terminal(ReactionStage::Failure),
                Effect::Post {
                    channel_id: request.channel_id.clone(),
                    text: format!("Ops agent returned an invalid result: {error}"),
                },
            ];
        }
    };
    record_turn(channels, channels_path, request, Ok(&parsed.reply));
    let bound_repo = bound_repo(&request.channel_id);
    let mut effects = vec![
        terminal(ReactionStage::Success),
        Effect::Post {
            channel_id: request.channel_id.clone(),
            text: with_repo_header(bound_repo.as_deref(), parsed.reply),
        },
    ];
    for action in parsed.actions {
        match action {
            Ok(command) => effects.push(Effect::Action {
                channel_id: request.channel_id.clone(),
                user_id: request.user_id.clone(),
                case_id: request.case.as_ref().map(|case| case.id.clone()),
                command,
            }),
            Err(reason) => effects.push(Effect::AuditRefusal {
                user_id: request.user_id.clone(),
                reason,
            }),
        }
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bound_channel_gets_a_repo_header() {
        assert_eq!(
            with_repo_header(Some("widgets"), "ok".into()),
            "**[widgets]**\nok"
        );
    }

    #[test]
    fn an_unbound_channel_gets_the_reply_unchanged() {
        assert_eq!(with_repo_header(None, "ok".into()), "ok");
    }
}
