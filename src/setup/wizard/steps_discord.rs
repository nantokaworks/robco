use std::io::{BufRead, Write};

use crate::{
    Error, Result,
    config::Config,
    overseer::{config::NotifyLevel, session::env as session_env},
};

use super::prompt;

/// Answer that clears an optional value. A blank answer keeps the current
/// value (`prompt::text` substitutes the default), so "unset" needs its own
/// spelling — every optional prompt label names both conventions.
const CLEAR: &str = "-";

pub(crate) fn discord<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    config: &mut Config,
) -> Result<()> {
    // Resolved before the mutable borrow below: the env file the token value
    // lands in is a sibling of `overseer.discord`, not part of it.
    let env_file = session_env::env_file_path(config);
    let discord = &mut config.overseer.discord;
    discord.enabled = prompt::confirm(input, output, "Configure Discord?", discord.enabled)?;
    if !discord.enabled {
        return Ok(());
    }
    discord.notify_level = notify_level(input, output, discord.notify_level)?;
    writeln!(
        output,
        "▌ robco ▸ NOTE ············· for the next answers: leave blank to keep the shown value, enter '-' to clear it"
    )?;
    discord.channel_id = optional_id(
        input,
        output,
        "Discord channel ID (leave blank to keep, enter '-' to clear)",
        discord.channel_id.as_deref().unwrap_or(""),
    )?;
    discord.notify_channel_id = optional_id(
        input,
        output,
        "Notify channel ID for reports (leave blank to keep, enter '-' to clear)",
        discord.notify_channel_id.as_deref().unwrap_or(""),
    )?;
    discord.allowed_user_ids = id_list(
        input,
        output,
        "Allowed user IDs (comma-separated; leave blank to keep, enter '-' to clear)",
        &discord.allowed_user_ids.join(","),
    )?;
    discord.chat_category_ids = id_list(
        input,
        output,
        "Chat category IDs (comma-separated; leave blank to keep, enter '-' to clear)",
        &discord.chat_category_ids.join(","),
    )?;
    if discord.channel_id.is_none() && discord.chat_category_ids.is_empty() {
        writeln!(
            output,
            "▌ robco ▸ WARN ············· Discord is enabled with no channel ID and no chat category IDs; the gateway has nothing to serve"
        )?;
    }
    discord.token_env = prompt::validated_text(
        input,
        output,
        "Discord token environment variable",
        &discord.token_env,
        "use an environment variable name, not a token value",
        env_name,
    )?;
    let token_env = discord.token_env.clone();
    let token = prompt::secret_text(
        input,
        output,
        "Discord bot token (leave blank to keep the current value)",
    )?;
    if token.is_empty() {
        return Ok(());
    }
    let path = env_file
        .ok_or_else(|| Error::Wizard("cannot resolve the session env file location".into()))?;
    session_env::write_var(&path, &token_env, &token)
}

/// Offers the notification-verbosity level, the sole gate for which Discord
/// events post — see `overseer::config::NotifyLevel::admits`.
fn notify_level<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    current: NotifyLevel,
) -> Result<NotifyLevel> {
    const LEVELS: [NotifyLevel; 4] = [
        NotifyLevel::Off,
        NotifyLevel::Errors,
        NotifyLevel::Summary,
        NotifyLevel::All,
    ];
    let choices = ["off", "errors", "summary", "all"].map(str::to_string);
    let default = LEVELS
        .iter()
        .position(|level| *level == current)
        .unwrap_or(0);
    let selected = prompt::select(
        input,
        output,
        "Discord notification level",
        &choices,
        default,
    )?;
    Ok(LEVELS[selected])
}

/// Prompt for one optional digit-only ID. Blank keeps `current` — which may
/// itself be "unset" — and the `-` sentinel clears the field.
fn optional_id<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    label: &str,
    current: &str,
) -> Result<Option<String>> {
    let answer = prompt::validated_text(
        input,
        output,
        label,
        current,
        "enter a digit-only ID, '-' to clear, or leave blank to keep",
        clear_or_digits,
    )?;
    Ok(match answer.as_str() {
        "" | CLEAR => None,
        _ => Some(answer),
    })
}

/// Prompt for a comma-separated digit-only ID list. Blank keeps `current` —
/// which may itself be empty — and the `-` sentinel clears the list.
fn id_list<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    label: &str,
    current: &str,
) -> Result<Vec<String>> {
    let answer = prompt::validated_text(
        input,
        output,
        label,
        current,
        "enter digit-only IDs, '-' to clear, or leave blank to keep",
        clear_or_id_list,
    )?;
    Ok(match answer.as_str() {
        "" | CLEAR => Vec::new(),
        ids => ids.split(',').map(|id| id.trim().to_string()).collect(),
    })
}

pub(crate) fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Valid answers for an optional-ID prompt: blank (keep), the clear
/// sentinel, or one digit-only ID.
pub(crate) fn clear_or_digits(value: &str) -> bool {
    value.is_empty() || value == CLEAR || digits(value)
}

/// Valid answers for an ID-list prompt: blank (keep), the clear sentinel,
/// or one or more digit-only IDs separated by commas.
pub(crate) fn clear_or_id_list(value: &str) -> bool {
    value.is_empty() || value == CLEAR || value.split(',').map(str::trim).all(digits)
}

pub(crate) fn env_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
