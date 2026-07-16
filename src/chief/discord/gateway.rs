use super::{
    actions::SystemExecutor,
    cursor::DecisionCursor,
    handler::Handler,
    ledger_requests::LedgerRequest,
    notifications::{Notification, from_decision},
};
use crate::chief::{config::DiscordConfig, decision_log_path, discord_cursor_path};
use serde_json::json;
use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_http::Client;
use twilight_model::{
    channel::message::embed::Embed,
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
                        if let Some(response) = handler.handle(
                            &message.channel_id.to_string(),
                            &message.author.id.to_string(),
                            &message.content,
                            now,
                            &mut executor,
                        ) {
                            if let Some(channel) = channel_id(&current) {
                                send_text(&http, channel, &response).await;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("chief: Discord gateway error (reconnecting): {error}"),
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
                for _ in 0..20 {
                    let Some(pending) = cursor.next().map_err(|error| error.to_string())? else {
                        break;
                    };
                    let notification = pending.entry.as_ref()
                        .and_then(|entry| from_decision(&current, entry));
                    let delivered = match (notification, channel_id(&current)) {
                        (Some(notification), Some(channel)) => {
                            send_embed(&http, channel, notification).await
                        }
                        (Some(_), None) => false,
                        (None, _) => true,
                    };
                    if !cursor.complete(pending, delivered)
                        .map_err(|error| error.to_string())? {
                        retry_at = tokio::time::Instant::now() + retry_delay;
                        retry_delay = (retry_delay * 2).min(Duration::from_secs(30));
                        break;
                    }
                    retry_delay = Duration::from_secs(1);
                }
            }
        }
    }
}

fn channel_id(config: &DiscordConfig) -> Option<Id<ChannelMarker>> {
    let raw = config.channel_id.as_deref()?.parse::<u64>().ok()?;
    (raw != 0).then(|| Id::new(raw))
}

async fn send_text(http: &Client, channel: Id<ChannelMarker>, text: &str) {
    let text: String = text.replace('@', "@\u{200b}").chars().take(1900).collect();
    if let Err(error) = http.create_message(channel).content(&text).await {
        eprintln!("chief: Discord response failed: {error}");
    }
}

async fn send_embed(http: &Client, channel: Id<ChannelMarker>, notification: Notification) -> bool {
    let embed: Embed = match serde_json::from_value(json!({
        "title": notification.title,
        "description": notification.description,
        "color": notification.color,
        "type": "rich"
    })) {
        Ok(embed) => embed,
        Err(error) => {
            eprintln!("chief: Discord embed construction failed: {error}");
            return false;
        }
    };
    match http.create_message(channel).embeds(&[embed]).await {
        Ok(_) => true,
        Err(error) => {
            eprintln!("chief: Discord notification failed: {error}");
            false
        }
    }
}

async fn audit_permissions(http: &Client) {
    let Ok(response) = http.current_user_application().await else {
        eprintln!("chief: warning: Discord bot permission audit request failed");
        return;
    };
    let Ok(application) = response.model().await else {
        eprintln!("chief: warning: Discord bot permission audit response was invalid");
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
        | Permissions::EMBED_LINKS
        | Permissions::READ_MESSAGE_HISTORY;
    match bits {
        Some(bits) if bits & !allowed.bits() != 0 => {
            eprintln!("chief: warning: Discord application requests excessive bot permissions");
        }
        None => eprintln!("chief: warning: Discord bot permissions could not be audited"),
        _ => {}
    }
}
