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

use super::ops_agent::{Effect, PendingSession, SessionRequest, SessionSpawner, react_effect};
use super::reactions::ReactionStage;
use std::collections::HashMap;

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

    /// Polls every active session, draining and reporting each that finished.
    pub fn poll(&mut self) -> Vec<Effect> {
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
            .flat_map(|(request, result)| effects_from_result(&request, result))
            .collect()
    }
}

fn effects_from_result(request: &SessionRequest, result: Result<Vec<u8>, String>) -> Vec<Effect> {
    let terminal = |stage| react_effect(&request.channel_id, &request.message_id, stage);
    let raw = match result {
        Ok(raw) => raw,
        Err(error) => {
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
            return vec![
                terminal(ReactionStage::Failure),
                Effect::Post {
                    channel_id: request.channel_id.clone(),
                    text: format!("Ops agent returned an invalid result: {error}"),
                },
            ];
        }
    };
    let mut effects = vec![
        terminal(ReactionStage::Success),
        Effect::Post {
            channel_id: request.channel_id.clone(),
            text: parsed.reply,
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
