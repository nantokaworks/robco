mod probe;
mod prompt;
mod steps;
mod steps_discord;
pub(crate) mod steps_service;

use std::io::{self, IsTerminal, Write};

use crate::{Error, Result, config::Config};

pub fn run() -> Result<()> {
    if !io::stdin().is_terminal() {
        return Err(Error::Wizard(
            "interactive input requires a terminal; use `robco install --target <t>`".into(),
        ));
    }
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    run_interactive(&mut input, &mut output)
}

fn run_interactive<R: io::BufRead, W: Write>(input: &mut R, output: &mut W) -> Result<()> {
    writeln!(output, "▌ robco ▸ setup")?;
    writeln!(output, "  █▀▄ █▀█ █▀▄ █▀▀ █▀█")?;
    writeln!(output, "  █▀▄ █▄█ █▄▀ █▄▄ █▄█")?;

    let probes = probe::run();
    probe::render(output, &probes)?;
    if probe::missing_required(&probes)
        && !prompt::confirm(
            input,
            output,
            "git or tmux is missing; continue anyway?",
            false,
        )?
    {
        return Err(Error::Wizard(
            "setup cancelled because prerequisites are missing".into(),
        ));
    }

    steps::registration(input, output)?;
    let mut config = Config::load()?;
    steps::ensure_agent_profiles(&mut config);
    steps::overseer(input, output, &mut config)?;
    steps_discord::discord(input, output, &mut config)?;
    let service = steps_service::configure(input, output, &config)?;
    writeln!(output, "▌ robco ▸ summary ··········· setup complete")?;
    config.save()?;
    if let Some(service) = service {
        service.apply(output)?;
    }
    steps_service::warn_if_service_down(output, &config)?;
    Ok(())
}

#[cfg(test)]
mod probe_tests;
#[cfg(test)]
mod prompt_tests;
#[cfg(test)]
mod steps_discord_tests;
#[cfg(test)]
mod steps_tests;
