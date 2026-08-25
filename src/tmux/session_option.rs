use crate::Result;

use super::{
    TmuxServer, command_output, command_unit,
    session::{exact, has_session},
};

pub(super) fn capture_session_option(
    server: &TmuxServer,
    session: &str,
    option: &str,
) -> Result<Option<String>> {
    let session = exact(session);
    let output = server
        .command()
        .args(["show-options", "-t", &session, "-q", option])
        .output()?;
    let presence = command_output(output, "tmux show-options")?;
    if presence.is_empty() {
        return Ok(None);
    }

    let output = server
        .command()
        .args(["show-options", "-t", &session, "-q", "-v", option])
        .output()?;
    let value = command_output(output, "tmux show-options")?;
    let value = value.trim_end_matches(['\r', '\n']).to_string();
    Ok(Some(value))
}

pub(super) fn set_session_option(
    server: &TmuxServer,
    session: &str,
    option: &str,
    value: &str,
) -> Result<()> {
    let output = server
        .command()
        .args(["set-option", "-t", &exact(session), option, value])
        .output()?;
    command_unit(output, "tmux set-option")
}

fn unset_session_option(server: &TmuxServer, session: &str, option: &str) -> Result<()> {
    let output = server
        .command()
        .args(["set-option", "-u", "-t", &exact(session), option])
        .output()?;
    command_unit(output, "tmux set-option -u")
}

fn restore_session_option(
    server: &TmuxServer,
    session: &str,
    option: &str,
    previous: Option<&str>,
) -> Result<()> {
    match previous {
        Some(value) => set_session_option(server, session, option, value),
        None => unset_session_option(server, session, option),
    }
}

pub(super) fn restore_session_option_if_present(
    server: &TmuxServer,
    session: &str,
    option: &str,
    previous: Option<&str>,
) -> Result<()> {
    if !has_session(server, session)? {
        return Ok(());
    }
    restore_session_option(server, session, option, previous)
}
