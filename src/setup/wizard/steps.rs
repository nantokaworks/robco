use std::io::{BufRead, Write};

use crate::{
    Result,
    cli::InstallTarget,
    config::{Config, Profile},
    setup::install_targets,
};

use super::prompt;

pub(crate) fn registration<R: BufRead, W: Write>(input: &mut R, output: &mut W) -> Result<()> {
    let choices = ["all", "claude", "codex", "openclaw", "skip"].map(str::to_string);
    let selected = prompt::select(input, output, "MCP client registration", &choices, 4)?;
    let targets: &[InstallTarget] = match selected {
        0 => &[
            InstallTarget::Claude,
            InstallTarget::Codex,
            InstallTarget::Openclaw,
        ],
        1 => &[InstallTarget::Claude],
        2 => &[InstallTarget::Codex],
        3 => &[InstallTarget::Openclaw],
        _ => &[],
    };
    if targets.is_empty() {
        writeln!(output, "▌ robco ▸ MCP ··············· skipped")?;
        Ok(())
    } else {
        install_targets(targets)
    }
}

pub(crate) fn chief<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    config: &mut Config,
) -> Result<()> {
    let default_program = config.default_program.clone();
    let profiles = config.profiles.clone();
    let chief = &mut config.chief;
    chief.dispatch_enabled = prompt::confirm(
        input,
        output,
        "Enable Chief dispatch?",
        chief.dispatch_enabled,
    )?;
    let auto_merge = prompt::confirm(input, output, "Enable auto-merge?", chief.auto_merge)?;
    if auto_merge && !chief.auto_merge {
        writeln!(
            output,
            "▌ robco ▸ WARN ············· branch protection and checks are required"
        )?;
    }
    chief.auto_merge = auto_merge;
    chief.worker_profile = profile(
        input,
        output,
        &default_program,
        &profiles,
        "Worker profile",
        &chief.worker_profile,
    )?;
    chief.triage_profile = profile(
        input,
        output,
        &default_program,
        &profiles,
        "Triage profile",
        &chief.triage_profile,
    )?;
    chief.max_workers =
        prompt::number(input, output, "Maximum workers", chief.max_workers, 0, 999)?;
    chief.per_repo_limit = prompt::number(
        input,
        output,
        "Per-repository worker limit",
        chief.per_repo_limit,
        0,
        999,
    )?;
    chief.daily_dispatch_limit = prompt::number(
        input,
        output,
        "Daily dispatch limit",
        chief.daily_dispatch_limit as usize,
        0,
        u32::MAX as usize,
    )? as u32;
    Ok(())
}

fn profile<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    default_program: &str,
    profiles: &[Profile],
    label: &str,
    current: &Option<String>,
) -> Result<Option<String>> {
    let mut choices = vec![format!("default_program ({default_program})")];
    let mut values = vec![None];
    choices.extend(profiles.iter().map(|profile| profile.name.clone()));
    values.extend(profiles.iter().map(|profile| Some(profile.name.clone())));
    let mut default = current
        .as_ref()
        .and_then(|name| profiles.iter().position(|profile| &profile.name == name))
        .map_or(0, |index| index + 1);
    if let Some(name) = current
        && default == 0
    {
        default = choices.len();
        choices.push(format!("{name} (current, unavailable)"));
        values.push(Some(name.clone()));
    }
    let selected = prompt::select(input, output, label, &choices, default)?;
    Ok(values[selected].clone())
}

pub(crate) fn discord<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    config: &mut Config,
) -> Result<()> {
    let discord = &mut config.chief.discord;
    discord.enabled = prompt::confirm(input, output, "Configure Discord?", discord.enabled)?;
    if !discord.enabled {
        return Ok(());
    }
    discord.channel_id = Some(prompt::validated_text(
        input,
        output,
        "Discord channel ID",
        discord.channel_id.as_deref().unwrap_or(""),
        "channel ID must contain digits only",
        digits,
    )?);
    let users = prompt::validated_text(
        input,
        output,
        "Allowed user IDs (comma-separated)",
        &discord.allowed_user_ids.join(","),
        "provide one or more digit-only IDs",
        valid_user_ids,
    )?;
    discord.allowed_user_ids = users.split(',').map(|id| id.trim().to_string()).collect();
    discord.token_env = prompt::validated_text(
        input,
        output,
        "Discord token environment variable",
        &discord.token_env,
        "use an environment variable name, not a token value",
        env_name,
    )?;
    Ok(())
}

pub(crate) fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn valid_user_ids(value: &str) -> bool {
    let ids: Vec<_> = value.split(',').map(str::trim).collect();
    !ids.is_empty() && ids.iter().all(|id| digits(id))
}

pub(crate) fn env_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
