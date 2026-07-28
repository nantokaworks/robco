use super::{
    actions::SystemExecutor,
    handler::Handler,
    ops_agent::{Effect, OpsAgent},
    reactions,
};
use crate::overseer::{
    logging::{self, DecisionEntry, DecisionKind},
    triage::ExceptionCase,
};
use std::collections::VecDeque;
use twilight_http::Client;
use twilight_model::{
    channel::ChannelType,
    id::{Id, marker::ChannelMarker},
};

pub(super) async fn process_effects(
    http: &Client,
    parent: Option<Id<ChannelMarker>>,
    effects: Vec<Effect>,
    ops: &mut OpsAgent,
    handler: &mut Handler,
    executor: &mut SystemExecutor,
    now_secs: u64,
) {
    let mut effects: VecDeque<_> = effects.into();
    while let Some(effect) = effects.pop_front() {
        match effect {
            Effect::OpenThread(case) => {
                let Some(parent) = parent else { continue };
                match reconcile_thread(http, parent, &case).await {
                    Ok(Some(thread_id)) => match ops.thread_opened(case, thread_id) {
                        Ok(effect) => effects.push_back(effect),
                        Err(error) => eprintln!("overseer: Discord thread mapping failed: {error}"),
                    },
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("overseer: Discord thread reconciliation failed: {error}")
                    }
                }
            }
            Effect::Opening {
                case_id,
                channel_id,
                text,
            } => {
                let delivered = if let Some(channel) = parse_channel(&channel_id) {
                    super::gateway::send_text(http, channel, &text).await
                } else {
                    false
                };
                if delivered && let Err(error) = ops.opening_delivered(&case_id) {
                    eprintln!("overseer: Discord opening marker failed: {error}");
                }
            }
            Effect::Post { channel_id, text } => {
                if let Some(channel) = parse_channel(&channel_id) {
                    let _ = super::gateway::send_text(http, channel, &text).await;
                }
            }
            Effect::Action {
                channel_id,
                user_id,
                case_id,
                command,
            } => {
                let handled = handler.submit_generated(
                    &channel_id,
                    &user_id,
                    case_id,
                    command,
                    now_secs,
                    executor,
                );
                effects.push_back(Effect::Post {
                    channel_id,
                    text: handled.response,
                });
            }
            Effect::AuditRefusal { user_id, reason } => audit_refusal(&user_id, &reason),
            Effect::React {
                channel_id,
                message_id,
                stage,
            } => {
                if let (Some(channel), Some(message)) = (
                    reactions::parse_id(&channel_id),
                    reactions::parse_id(&message_id),
                ) {
                    reactions::react(http, channel, message, stage).await;
                }
            }
            Effect::DeliverResolution {
                thread_id,
                case_id,
                post,
                archive,
            } => {
                let Some(channel) = parse_channel(&thread_id) else {
                    continue;
                };
                let posted = !post
                    || super::gateway::send_text(
                        http,
                        channel,
                        &format!(
                            "Resolved case `{case_id}`; the answer was delivered to the worker."
                        ),
                    )
                    .await;
                let archived = !archive
                    || posted
                        && match http.update_thread(channel).archived(true).await {
                            Ok(_) => true,
                            Err(error) => {
                                eprintln!("overseer: Discord thread archive failed: {error}");
                                false
                            }
                        };
                match ops.resolution_delivery(&case_id, posted, archived) {
                    Ok(super::ops_state::DeliveryOutcome::Exhausted) => {
                        match audit_delivery_exhausted(&case_id, posted, archived) {
                            Ok(()) => {
                                if let Err(error) = ops.finalize_exhausted_resolution(&case_id) {
                                    eprintln!("overseer: exhaustion marker failed: {error}");
                                }
                            }
                            Err(error) => eprintln!(
                                "overseer: failed to audit resolution delivery exhaustion: {error}"
                            ),
                        }
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("overseer: resolution delivery marker failed: {error}"),
                }
            }
        }
    }
}

async fn reconcile_thread(
    http: &Client,
    parent: Id<ChannelMarker>,
    case: &ExceptionCase,
) -> Result<Option<String>, String> {
    let name = thread_name(&case.id);
    let parent_model = http
        .channel(parent)
        .await
        .map_err(|error| error.to_string())?
        .model()
        .await
        .map_err(|error| error.to_string())?;
    let guild = parent_model.guild_id.ok_or("Discord parent has no guild")?;
    let active = http
        .active_threads(guild)
        .await
        .map_err(|error| error.to_string())?
        .model()
        .await
        .map_err(|error| error.to_string())?;
    if let Some(thread) = active
        .threads
        .iter()
        .find(|thread| thread.parent_id == Some(parent) && thread.name.as_deref() == Some(&name))
    {
        return Ok(Some(thread.id.to_string()));
    }
    let archived = http
        .public_archived_threads(parent)
        .limit(100)
        .await
        .map_err(|error| error.to_string())?
        .model()
        .await
        .map_err(|error| error.to_string())?;
    if let Some(thread) = archived
        .threads
        .iter()
        .find(|thread| thread.name.as_deref() == Some(&name))
    {
        return Ok(Some(thread.id.to_string()));
    }
    let thread = http
        .create_thread(parent, &name, ChannelType::PublicThread)
        .await
        .map_err(|error| error.to_string())?
        .model()
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(thread.id.to_string()))
}

fn thread_name(case_id: &str) -> String {
    format!("escalation-{case_id}").chars().take(90).collect()
}

fn parse_channel(raw: &str) -> Option<Id<ChannelMarker>> {
    raw.parse::<u64>()
        .ok()
        .filter(|value| *value != 0)
        .map(Id::new)
}

fn audit_refusal(user_id: &str, reason: &str) {
    let mut entry = DecisionEntry::new(
        DecisionKind::Hold,
        format!("conversational action refused: {reason}"),
    );
    entry.source = Some("discord".into());
    entry.user_id = Some(user_id.into());
    if let Err(error) = logging::append(&entry) {
        eprintln!("overseer: failed to audit Discord refusal: {error}");
    }
}

fn audit_delivery_exhausted(case_id: &str, posted: bool, archived: bool) -> Result<(), String> {
    record_delivery_exhausted(case_id, posted, archived, |entry| {
        logging::append(entry).map_err(|error| error.to_string())
    })
}

fn record_delivery_exhausted(
    case_id: &str,
    posted: bool,
    archived: bool,
    append: impl FnOnce(&DecisionEntry) -> Result<(), String>,
) -> Result<(), String> {
    let mut entry = DecisionEntry::new(
        DecisionKind::Hold,
        format!(
            "Discord resolution delivery exhausted for {case_id}: posted={posted} archived={archived}"
        ),
    );
    entry.source = Some("discord".into());
    append(&entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_resolution_delivery_records_explicit_decision() {
        let mut recorded = None;
        record_delivery_exhausted("case-1", false, true, |entry| {
            recorded = Some(entry.clone());
            Ok(())
        })
        .unwrap();
        let entry = recorded.unwrap();
        assert_eq!(entry.source.as_deref(), Some("discord"));
        assert!(entry.reason.contains("case-1"));
        assert!(entry.reason.contains("posted=false archived=true"));
    }
}
