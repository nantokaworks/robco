use super::{commands::Command, reactions::ReactionStage};
use crate::overseer::triage::ExceptionCase;

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
    React {
        channel_id: String,
        message_id: String,
        stage: ReactionStage,
    },
}

/// Outcome of routing an inbound message to the conversational ops agent.
/// Distinct from a plain `Vec<Effect>` so the gateway can tell an ignored
/// message (no typing indicator, no reaction) apart from one that started a
/// session (typing indicator, reply arriving later). Both accepted variants
/// carry the reaction trail the acceptance itself produces, so the caller
/// never has to re-derive which stage the message reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RouteOutcome {
    /// The message was not directed at the ops agent; produce no reply,
    /// show no typing indicator, and post no reaction.
    Ignored,
    /// A session was spawned; its reply arrives later via `poll`. The
    /// effects are the acknowledged/working reactions alone.
    Started(Vec<Effect>),
    /// An immediate reply is due (busy, at capacity, or spawn failure),
    /// carried here alongside the reactions that accompany it.
    Immediate(Vec<Effect>),
}

pub(super) fn react_effect(channel_id: &str, message_id: &str, stage: ReactionStage) -> Effect {
    Effect::React {
        channel_id: channel_id.into(),
        message_id: message_id.into(),
        stage,
    }
}
