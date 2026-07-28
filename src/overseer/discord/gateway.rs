use super::{
    actions::SystemExecutor,
    category::{self, CategoryCache},
    cursor::DecisionCursor,
    handler::Handler,
    ledger_requests::LedgerRequest,
    localize::{LocalizeSpawner, SystemLocalizeSpawner, TitleCache},
    notify::{self, InFlight},
    ops_agent::{OpsAgent, RouteOutcome},
    ops_gateway::process_effects,
    ops_session::SystemSessionSpawner,
    reactions,
    typing::TypingKeepalive,
};
use crate::overseer::{
    config::DiscordConfig, decision_log_path, discord_cursor_path, discord_ops_dir, triage_dir,
};
use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_http::Client;
use twilight_model::{
    guild::Permissions,
    id::{Id, marker::ChannelMarker},
};

pub async fn run(
    config: &Arc<RwLock<DiscordConfig>>,
    token: String,
    stop: &AtomicBool,
    handler: &mut Handler,
    ledger_requests: &Sender<LedgerRequest>,
) -> Result<(), String> {
    let http = Client::new(token.clone());
    audit_permissions(&http).await;
    let intents = Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT;
    let mut shard = Shard::new(ShardId::ONE, token, intents);
    let mut executor = SystemExecutor::new(ledger_requests.clone());
    let mut cursor = DecisionCursor::load(
        decision_log_path().map_err(|error| error.to_string())?,
        discord_cursor_path().map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let ops_root = discord_ops_dir().map_err(|error| error.to_string())?;
    let initial = config
        .read()
        .unwrap_or_else(|lock| lock.into_inner())
        .clone();
    let mut ops = OpsAgent::load(
        initial.channel_id.unwrap_or_default(),
        initial.allowed_user_ids,
        triage_dir().map_err(|error| error.to_string())?,
        ops_root.join("threads.json"),
    )?;
    let mut spawner = SystemSessionSpawner::new(ops_root.join("sessions"));
    let mut localize_spawner: Box<dyn LocalizeSpawner> =
        Box::new(SystemLocalizeSpawner::new(ops_root.join("localize")));
    let mut localize_cache = TitleCache::default();
    let mut in_flight: Option<InFlight> = None;
    let mut category_cache = CategoryCache::default();
    let mut typing = TypingKeepalive::default();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut retry_at = tokio::time::Instant::now();
    let mut retry_delay = Duration::from_secs(1);
    loop {
        tokio::select! {
            item = shard.next_event(EventTypeFlags::MESSAGE_CREATE) => {
                let Some(item) = item else { return Ok(()); };
                match item {
                    Ok(Event::MessageCreate(message)) if !message.author.bot => {
                        let current = config.read()
                            .unwrap_or_else(|lock| lock.into_inner()).clone();
                        handler.update_config(&current);
                        let now = SystemTime::now().duration_since(UNIX_EPOCH)
                            .unwrap_or_default().as_secs();
                        let message_channel = message.channel_id.to_string();
                        let message_id = message.id.to_string();
                        let thread = ops.is_thread(&message_channel);
                        // A category lookup is only worth the HTTP round trip for a
                        // channel that is neither the parent channel nor an already
                        // known ops thread — both already grant `extra_channel` on
                        // their own, and an empty `chat_category_ids` short-circuits
                        // `is_in_category` before it touches the cache regardless.
                        let category_member = if thread
                            || message_channel == current.channel_id.clone().unwrap_or_default()
                        {
                            false
                        } else {
                            category::is_in_category(
                                &mut category_cache,
                                |channel_id| fetch_parent_category(&http, channel_id),
                                &message_channel,
                                &current.chat_category_ids,
                                Instant::now(),
                            ).await
                        };
                        let extra_channel = thread || category_member;
                        if let Some(handled) = handler.handle_allowed(
                            &message_channel,
                            &message.author.id.to_string(),
                            &message.content,
                            now,
                            extra_channel,
                            &mut executor,
                        ) {
                            // Typing first, so the indicator appears with the
                            // least latency; the reaction trail follows.
                            typing.start(&http, &message_channel, message.channel_id).await;
                            for stage in reactions::handled_stages(handled.outcome) {
                                reactions::react(&http, message.channel_id, message.id, stage).await;
                            }
                            let _ = send_text(&http, message.channel_id, &handled.response).await;
                            if thread && handled.succeeded {
                                let effects = if let Some(command) = handled.executed.as_ref() {
                                    ops.resolve_answer(
                                        &message_channel,
                                        handled.case_id.as_deref(),
                                        command,
                                    )?
                                } else {
                                    Vec::new()
                                };
                                process_effects(
                                    &http, notify::channel_id(&current), effects, &mut ops,
                                    handler, &mut executor, now,
                                ).await;
                            }
                        } else {
                            match ops.route(
                                &message_channel,
                                &message.author.id.to_string(),
                                &message.content,
                                &message_id,
                                &mut spawner,
                                category_member,
                            ) {
                                RouteOutcome::Ignored => {}
                                // Both accepted outcomes carry their reaction
                                // trail, so they are driven identically here;
                                // the variants stay distinct because only
                                // `Immediate` also carries a reply to post.
                                RouteOutcome::Started(effects)
                                | RouteOutcome::Immediate(effects) => {
                                    typing.start(&http, &message_channel, message.channel_id).await;
                                    process_effects(
                                        &http, notify::channel_id(&current), effects, &mut ops,
                                        handler, &mut executor, now,
                                    ).await;
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("overseer: Discord gateway error (reconnecting): {error}"),
                }
            }
            _ = tick.tick() => {
                if stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                if tokio::time::Instant::now() < retry_at {
                    continue;
                }
                let current = config.read()
                    .unwrap_or_else(|lock| lock.into_inner()).clone();
                handler.update_config(&current);
                ops.update_access(
                    current.channel_id.clone().unwrap_or_default(),
                    current.allowed_user_ids.clone(),
                    current.chat_concurrency_cap,
                );
                let mut effects = ops.discover()?;
                effects.extend(ops.poll());
                typing.reconcile(&http, ops.active_chat_channels()).await;
                let now = SystemTime::now().duration_since(UNIX_EPOCH)
                    .unwrap_or_default().as_secs();
                process_effects(
                    &http, notify::channel_id(&current), effects, &mut ops,
                    handler, &mut executor, now,
                ).await;
                notify::deliver(
                    &http, &current, &mut cursor, &mut *localize_spawner, &mut localize_cache,
                    &mut in_flight, &mut retry_at, &mut retry_delay,
                ).await?;
            }
        }
    }
}

/// The live half of `category::is_in_category`'s injected lookup: resolves
/// one channel's parent category over Discord's HTTP API, following the
/// same `channel(id).model()` pattern `ops_gateway::reconcile_thread`
/// already uses for thread lookups.
async fn fetch_parent_category(
    http: &Client,
    channel_id: String,
) -> Result<Option<String>, String> {
    let id: u64 = channel_id
        .parse()
        .map_err(|error: std::num::ParseIntError| error.to_string())?;
    let channel = http
        .channel(Id::new(id))
        .await
        .map_err(|error| error.to_string())?
        .model()
        .await
        .map_err(|error| error.to_string())?;
    Ok(channel.parent_id.map(|id| id.to_string()))
}

pub(super) async fn send_text(http: &Client, channel: Id<ChannelMarker>, text: &str) -> bool {
    let text: String = text.replace('@', "@\u{200b}").chars().take(1900).collect();
    match http.create_message(channel).content(&text).await {
        Ok(_) => true,
        Err(error) => {
            eprintln!("overseer: Discord response failed: {error}");
            false
        }
    }
}

async fn audit_permissions(http: &Client) {
    let Ok(response) = http.current_user_application().await else {
        eprintln!("overseer: warning: Discord bot permission audit request failed");
        return;
    };
    let Ok(application) = response.model().await else {
        eprintln!("overseer: warning: Discord bot permission audit response was invalid");
        return;
    };
    let Ok(value) = serde_json::to_value(application) else {
        return;
    };
    let bits = value
        .pointer("/install_params/permissions")
        .and_then(|value| {
            value
                .as_str()
                .and_then(|raw| raw.parse::<u64>().ok())
                .or_else(|| value.as_u64())
        });
    let allowed = Permissions::VIEW_CHANNEL
        | Permissions::SEND_MESSAGES
        | Permissions::SEND_MESSAGES_IN_THREADS
        | Permissions::CREATE_PUBLIC_THREADS
        | Permissions::MANAGE_THREADS
        | Permissions::EMBED_LINKS
        | Permissions::READ_MESSAGE_HISTORY
        | Permissions::ADD_REACTIONS;
    match bits {
        Some(bits) if bits & !allowed.bits() != 0 => {
            eprintln!("overseer: warning: Discord application requests excessive bot permissions");
        }
        None => eprintln!("overseer: warning: Discord bot permissions could not be audited"),
        _ => {}
    }
}
