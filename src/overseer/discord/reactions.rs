//! Emoji progress reactions on a user's triggering Discord message.
//!
//! Reactions accumulate rather than replace: `Acknowledged` fires as soon as
//! a message is accepted, `Working` when a conversational session actually
//! starts, and exactly one terminal reaction closes the trail. `react`
//! returns nothing a caller could act on, so a failed or permission-denied
//! request can never affect the reply path it runs alongside; the missing-
//! permission case is logged once (not per message) to avoid a log storm on
//! every subsequent reaction attempt.

use super::handler::HandledOutcome;
use std::sync::atomic::{AtomicBool, Ordering};
use twilight_http::{Client, error::ErrorType, request::channel::reaction::RequestReactionType};
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReactionStage {
    Acknowledged,
    Working,
    Success,
    Failure,
    Refused,
}

impl ReactionStage {
    fn emoji(self) -> &'static str {
        match self {
            ReactionStage::Acknowledged => "\u{1F440}",   // 👀
            ReactionStage::Working => "\u{2699}\u{FE0F}", // ⚙️
            ReactionStage::Success => "\u{2705}",         // ✅
            ReactionStage::Failure => "\u{274C}",         // ❌
            ReactionStage::Refused => "\u{1F6AB}",        // 🚫
        }
    }
}

/// Reaction stages for a synchronously-handled `!command` / `CONFIRM`
/// message. `AwaitingConfirmation` gets only the acknowledgement: the
/// command has not run yet, so no terminal reaction is due until the
/// `CONFIRM` reply lands and resolves to one of the other outcomes.
pub(super) fn handled_stages(outcome: HandledOutcome) -> Vec<ReactionStage> {
    match outcome {
        HandledOutcome::AwaitingConfirmation => vec![ReactionStage::Acknowledged],
        HandledOutcome::Success => vec![ReactionStage::Acknowledged, ReactionStage::Success],
        HandledOutcome::Failure => vec![ReactionStage::Acknowledged, ReactionStage::Failure],
        HandledOutcome::Refused => vec![ReactionStage::Acknowledged, ReactionStage::Refused],
    }
}

pub(super) fn parse_id<T>(raw: &str) -> Option<Id<T>> {
    raw.parse::<u64>()
        .ok()
        .filter(|value| *value != 0)
        .map(Id::new)
}

/// Adds one reaction to a Discord message. Best-effort: any failure is
/// logged and swallowed here rather than returned, so a missing permission,
/// a deleted message, or a transient API error can never delay or block the
/// reply this runs alongside.
pub(super) async fn react(
    http: &Client,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    stage: ReactionStage,
) {
    let emoji = RequestReactionType::Unicode {
        name: stage.emoji(),
    };
    if let Err(error) = http.create_reaction(channel_id, message_id, &emoji).await {
        if is_missing_permission(&error) {
            if PERMISSION_WARNING.should_warn() {
                eprintln!(
                    "overseer: warning: Discord bot lacks the ADD_REACTIONS permission; \
                     grant it to the bot's role so progress reactions can be posted"
                );
            }
        } else {
            eprintln!("overseer: Discord reaction failed ({stage:?}): {error}");
        }
    }
}

fn is_missing_permission(error: &twilight_http::Error) -> bool {
    matches!(
        error.kind(),
        ErrorType::Response { status, .. } if status.get() == 403
    )
}

struct PermissionWarning(AtomicBool);

impl PermissionWarning {
    const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    fn should_warn(&self) -> bool {
        !self.0.swap(true, Ordering::Relaxed)
    }
}

static PERMISSION_WARNING: PermissionWarning = PermissionWarning::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_has_a_distinct_emoji() {
        let stages = [
            ReactionStage::Acknowledged,
            ReactionStage::Working,
            ReactionStage::Success,
            ReactionStage::Failure,
            ReactionStage::Refused,
        ];
        let mut emojis: Vec<_> = stages.iter().map(|stage| stage.emoji()).collect();
        emojis.sort_unstable();
        emojis.dedup();
        assert_eq!(emojis.len(), stages.len());
    }

    #[test]
    fn awaiting_confirmation_gets_only_the_acknowledgement() {
        assert_eq!(
            handled_stages(HandledOutcome::AwaitingConfirmation),
            vec![ReactionStage::Acknowledged]
        );
    }

    #[test]
    fn terminal_outcomes_accumulate_after_the_acknowledgement_in_order() {
        assert_eq!(
            handled_stages(HandledOutcome::Success),
            vec![ReactionStage::Acknowledged, ReactionStage::Success]
        );
        assert_eq!(
            handled_stages(HandledOutcome::Failure),
            vec![ReactionStage::Acknowledged, ReactionStage::Failure]
        );
        assert_eq!(
            handled_stages(HandledOutcome::Refused),
            vec![ReactionStage::Acknowledged, ReactionStage::Refused]
        );
    }

    #[test]
    fn permission_warning_fires_once() {
        let warning = PermissionWarning::new();
        assert!(warning.should_warn());
        assert!(!warning.should_warn());
        assert!(!warning.should_warn());
    }

    #[test]
    fn zero_and_unparseable_ids_are_rejected() {
        assert!(parse_id::<ChannelMarker>("0").is_none());
        assert!(parse_id::<ChannelMarker>("not-a-number").is_none());
        assert_eq!(parse_id::<ChannelMarker>("123").unwrap().get(), 123);
    }
}
