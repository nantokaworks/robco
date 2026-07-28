use super::{
    commands::Command,
    ops_messages::{capture_summary, opening_message},
    ops_sessions::{SessionSlots, StartOutcome},
    ops_state::{DeliveryOutcome, OpsState, ResolutionState},
};
use crate::overseer::triage::ExceptionCase;
use std::path::PathBuf;

/// Concurrent conversational sessions `OpsAgent` runs before further routing
/// gets the busy message. Each session is a spawned OS thread running an
/// agent CLI, so this bounds real resource use, not just Discord noise; see
/// `DiscordConfig::chat_concurrency_cap` for the operator-facing knob.
const DEFAULT_CONCURRENCY_CAP: usize = 3;

pub(super) trait PendingSession: Send {
    fn poll(&mut self) -> Option<Result<Vec<u8>, String>>;
}

pub(super) trait SessionSpawner {
    fn spawn(&mut self, request: SessionRequest) -> Result<Box<dyn PendingSession>, String>;
}

#[derive(Debug, Clone)]
pub(super) struct SessionRequest {
    pub user_id: String,
    pub channel_id: String,
    pub case: Option<ExceptionCase>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Effect {
    OpenThread(ExceptionCase),
    Opening {
        case_id: String,
        channel_id: String,
        text: String,
    },
    Post {
        channel_id: String,
        text: String,
    },
    Action {
        channel_id: String,
        user_id: String,
        case_id: Option<String>,
        command: Command,
    },
    AuditRefusal {
        user_id: String,
        reason: String,
    },
    DeliverResolution {
        thread_id: String,
        case_id: String,
        post: bool,
        archive: bool,
    },
}

pub(super) struct OpsAgent {
    parent_channel: String,
    allowed_users: Vec<String>,
    triage_root: PathBuf,
    state_path: PathBuf,
    state: OpsState,
    sessions: SessionSlots,
}

impl OpsAgent {
    pub fn load(
        parent_channel: String,
        allowed_users: Vec<String>,
        triage_root: PathBuf,
        state_path: PathBuf,
    ) -> Result<Self, String> {
        let state = OpsState::load(&state_path)?;
        Ok(Self {
            parent_channel,
            allowed_users,
            triage_root,
            state_path,
            state,
            sessions: SessionSlots::new(DEFAULT_CONCURRENCY_CAP),
        })
    }

    pub fn update_access(
        &mut self,
        parent_channel: String,
        allowed_users: Vec<String>,
        concurrency_cap: usize,
    ) {
        self.parent_channel = parent_channel;
        self.allowed_users = allowed_users;
        self.sessions.set_cap(concurrency_cap);
    }

    pub fn discover(&mut self) -> Result<Vec<Effect>, String> {
        self.state
            .reconcile_cases(&self.state_path, &self.triage_root)?;
        let mut effects = Vec::new();
        for entry in self.state.cases() {
            let Some(thread_id) = entry.thread_id.as_ref() else {
                effects.push(Effect::OpenThread(entry.case.clone()));
                continue;
            };
            if !entry.opening_delivered {
                let capture = capture_summary(&self.triage_root, &entry.case.id);
                effects.push(Effect::Opening {
                    case_id: entry.case.id.clone(),
                    channel_id: thread_id.clone(),
                    text: opening_message(&entry.case, &capture),
                });
            }
            if entry.resolution == ResolutionState::Pending {
                effects.push(Effect::DeliverResolution {
                    thread_id: thread_id.clone(),
                    case_id: entry.case.id.clone(),
                    post: !entry.resolution_posted,
                    archive: !entry.archived,
                });
            }
        }
        Ok(effects)
    }

    pub fn thread_opened(
        &mut self,
        case: ExceptionCase,
        thread_id: String,
    ) -> Result<Effect, String> {
        let capture = capture_summary(&self.triage_root, &case.id);
        self.state
            .map_thread(&self.state_path, &case.id, thread_id.clone())?;
        Ok(Effect::Opening {
            case_id: case.id.clone(),
            channel_id: thread_id,
            text: opening_message(&case, &capture),
        })
    }

    pub fn opening_delivered(&mut self, case_id: &str) -> Result<(), String> {
        self.state.mark_opening_delivered(&self.state_path, case_id)
    }

    pub fn is_thread(&self, channel_id: &str) -> bool {
        self.state.by_thread(channel_id).is_some()
    }

    /// `category_member` is resolved by the caller (`gateway.rs`, via
    /// `category::is_in_category`) since it requires a Discord HTTP fetch
    /// this synchronous, unit-testable routing logic has no business making.
    pub fn route(
        &mut self,
        channel_id: &str,
        user_id: &str,
        message: &str,
        spawner: &mut dyn SessionSpawner,
        category_member: bool,
    ) -> Option<Effect> {
        if !self.allowed_users.iter().any(|allowed| allowed == user_id)
            || (channel_id != self.parent_channel
                && !self.is_thread(channel_id)
                && !category_member)
            || message.trim().is_empty()
            || message.trim_start().starts_with('!')
            || message.trim_start().starts_with("CONFIRM ")
        {
            return None;
        }
        let case = self
            .state
            .by_thread(channel_id)
            .map(|entry| entry.case.clone());
        let request = SessionRequest {
            user_id: user_id.into(),
            channel_id: channel_id.into(),
            case,
            message: message.into(),
        };
        let busy = Effect::Post {
            channel_id: channel_id.into(),
            text: "The ops agent is handling another request; please retry shortly.".into(),
        };
        match self.sessions.start(channel_id, request, spawner) {
            StartOutcome::Started => None,
            StartOutcome::Busy | StartOutcome::AtCapacity => Some(busy),
            StartOutcome::SpawnFailed(error) => Some(Effect::Post {
                channel_id: channel_id.into(),
                text: format!("Ops agent could not start: {error}"),
            }),
        }
    }

    pub fn poll(&mut self) -> Vec<Effect> {
        self.sessions.poll()
    }

    pub fn resolve_answer(
        &mut self,
        thread_id: &str,
        bound_case_id: Option<&str>,
        command: &Command,
    ) -> Result<Vec<Effect>, String> {
        let Command::Answer { agent, .. } = command else {
            return Ok(Vec::new());
        };
        let Some(entry) = self.state.by_thread(thread_id) else {
            return Ok(Vec::new());
        };
        if agent != &entry.case.worker_id
            || bound_case_id.is_some_and(|case_id| case_id != entry.case.id)
        {
            return Ok(Vec::new());
        }
        let case_id = entry.case.id.clone();
        if !self.state.begin_resolution(&self.state_path, &case_id)? {
            return Ok(Vec::new());
        }
        Ok(vec![Effect::DeliverResolution {
            thread_id: thread_id.into(),
            case_id,
            post: true,
            archive: true,
        }])
    }

    pub fn resolution_delivery(
        &mut self,
        case_id: &str,
        posted: bool,
        archived: bool,
    ) -> Result<DeliveryOutcome, String> {
        self.state
            .record_delivery(&self.state_path, case_id, posted, archived)
    }

    pub fn finalize_exhausted_resolution(&mut self, case_id: &str) -> Result<(), String> {
        self.state.finalize_exhausted(&self.state_path, case_id)
    }
}

#[cfg(test)]
#[path = "ops_agent_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ops_lifecycle_tests.rs"]
mod lifecycle_tests;

#[cfg(test)]
#[path = "ops_agent_category_tests.rs"]
mod category_tests;
