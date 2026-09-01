use std::io::{BufRead, Write};

use crate::{
    Result,
    cli::InstallTarget,
    config::{Config, Profile, default_profiles, resolve_program},
    setup::install_targets,
};

use super::prompt;

/// Adds a built-in profile for each agent CLI that is installed but has no
/// profile in the config yet. A config written before profiles existed, or
/// saved with `profiles: []`, never offers codex in the wizard even when the
/// codex CLI is installed — this repairs that before the profile steps run.
/// The profile matching `default_program` is skipped because the wizard's
/// `default_program (...)` row already covers it.
pub(crate) fn ensure_agent_profiles(config: &mut Config) {
    ensure_agent_profiles_with(config, |program| resolve_program(program).is_some());
}

pub(crate) fn ensure_agent_profiles_with(config: &mut Config, installed: impl Fn(&str) -> bool) {
    for builtin in default_profiles() {
        let covered = builtin.name == config.default_program
            || config
                .profiles
                .iter()
                .any(|profile| profile.name == builtin.name || profile.program == builtin.program);
        if !covered && installed(&builtin.program) {
            config.profiles.push(builtin);
        }
    }
}

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

pub(crate) fn overseer<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    config: &mut Config,
) -> Result<()> {
    let default_program = config.default_program.clone();
    let profiles = config.profiles.clone();
    let overseer = &mut config.overseer;
    writeln!(
        output,
        "▌ robco ▸ INFO ············· a merge still needs a per-PR approval \
         (`m`, `!merge`, or `robco_approve`); this only lets the daemon act on one"
    )?;
    let auto_merge = prompt::confirm(input, output, "Enable the merge gate?", overseer.auto_merge)?;
    if auto_merge && !overseer.auto_merge {
        writeln!(
            output,
            "▌ robco ▸ WARN ············· branch protection and checks are required"
        )?;
    }
    overseer.auto_merge = auto_merge;
    overseer.worker_profile = profile(
        input,
        output,
        &default_program,
        &profiles,
        "Worker profile",
        &overseer.worker_profile,
    )?;
    overseer.triage_profile = profile(
        input,
        output,
        &default_program,
        &profiles,
        "Triage profile",
        &overseer.triage_profile,
    )?;
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
