use std::{io::Write, process::Stdio, thread, time::Duration};

use crate::{Error, Result};

use super::{
    TmuxServer, command_output, command_unit,
    session::{has_server, has_session, sanitize_target_part},
    session_option::{
        capture_session_option, restore_session_option_if_present, set_session_option,
    },
};

pub(super) struct ReturnKeyBinding {
    server: TmuxServer,
    session: String,
    previous: Option<String>,
    previous_status: Option<String>,
    previous_status_right: Option<String>,
}

impl ReturnKeyBinding {
    pub(super) fn install(server: &TmuxServer, in_tmux: bool, session: &str) -> Result<Self> {
        let previous = capture_key_binding(server, "C-q")?;
        let mut command = server.command();
        if in_tmux {
            // One idempotent root binding that works at any nesting depth. Both
            // the return target and the return signal are derived from the
            // session the key is pressed in, expanded by tmux at the press:
            // - `#{@robco_return}` is a per-session user option each robco sets to
            //   the session its TUI lives in (see below), so C-q returns exactly
            //   one level up. This must NOT be `switch-client -l`: `-l` is a
            //   single shared "last session" pointer, not a stack, so nested
            //   robco returns scramble it and a parent's C-q ends up on a stray
            //   session (and can even leave C-q unbound). The per-session option
            //   composes correctly at any depth. `-l` remains only as a fallback
            //   when the option is unset.
            // - `#{session_name}` scopes the wait-for signal so every nested
            //   robco waits on its own session's signal.
            // The binding text is identical at every level (the per-session
            // option carries the per-level difference), so a deeper instance
            // installing over a shallower one cannot corrupt it.
            command.args([
                "bind-key",
                "-T",
                "root",
                "C-q",
                "run-shell",
                "tmux switch-client -t '#{@robco_return}' 2>/dev/null || tmux switch-client -l ; tmux wait-for -S robco-return-#{session_name}",
            ]);
        } else {
            // Outside tmux the client returns when the blocking `tmux attach`
            // exits, so detaching is enough; no wait-for signal is needed.
            command.args(["bind-key", "-T", "root", "C-q", "detach-client"]);
        }
        let output = command.output()?;
        command_unit(output, "tmux bind-key")?;
        if in_tmux {
            // Record the session C-q should return to for this target: the
            // session the client is currently on, i.e. this robco instance's
            // TUI. Stored on the target session so the C-q binding above can
            // switch back exactly one level. Best-effort — on failure the
            // binding falls back to `switch-client -l`.
            match current_client_session(server) {
                Ok(origin) if !origin.is_empty() => {
                    let _ = set_session_option(server, session, "@robco_return", &origin);
                }
                _ => {}
            }
        }
        let (previous_status, previous_status_right) = match (|| {
            let previous_status = capture_session_option(server, session, "status")?;
            let previous_status_right = capture_session_option(server, session, "status-right")?;
            set_session_option(server, session, "status", "on")?;
            set_session_option(server, session, "status-right", "C-q to return")?;
            Ok((previous_status, previous_status_right))
        })() {
            Ok(previous) => previous,
            Err(err) => {
                let _ = restore_key_binding(server, previous.as_deref());
                return Err(err);
            }
        };
        Ok(Self {
            server: server.clone(),
            session: session.to_string(),
            previous,
            previous_status,
            previous_status_right,
        })
    }

    pub(super) fn restore(self) -> Result<()> {
        let key_result = if has_server(&self.server)? {
            restore_key_binding(&self.server, self.previous.as_deref())
        } else {
            Ok(())
        };
        let status_result = restore_session_option_if_present(
            &self.server,
            &self.session,
            "status",
            self.previous_status.as_deref(),
        );
        let status_right_result = restore_session_option_if_present(
            &self.server,
            &self.session,
            "status-right",
            self.previous_status_right.as_deref(),
        );
        key_result.and(status_result.and(status_right_result))
    }
}

fn current_client_session(server: &TmuxServer) -> Result<String> {
    let output = server
        .command()
        .args(["display-message", "-p", "#{client_session}"])
        .output()?;
    let value = command_output(output, "tmux display-message")?;
    Ok(value.trim().to_string())
}

fn capture_key_binding(server: &TmuxServer, key: &str) -> Result<Option<String>> {
    // The key-filtered form `list-keys -T root <key>` prints nothing on some
    // tmux builds (observed on 3.7), which would make this always report "no
    // previous binding". Restore would then unbind C-q even when a parent robco
    // still needs it — breaking C-q after a nested robco exits. List the whole
    // root table and find the key ourselves so save/restore works across
    // versions. Each line is a re-sourceable `bind-key -T root <key> ...`
    // command, so the matched line can be replayed verbatim by `source-file`.
    let output = server
        .command()
        .args(["list-keys", "-T", "root"])
        .output()?;
    let listing = command_output(output, "tmux list-keys")?;
    let binding = listing
        .lines()
        .find(|line| root_binding_key(line) == Some(key))
        .map(|line| line.trim().to_string());
    Ok(binding)
}

// Extract the key a `bind-key ... -T root <key> ...` listing line binds. Returns
// the token immediately after `-T root`; None if the line is not a root-table
// binding. Stops at the first `-T root`, so a `-T root` occurring inside the
// bound command's quoted body cannot be mistaken for the key column.
fn root_binding_key(line: &str) -> Option<&str> {
    let mut tokens = line.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "-T" {
            if tokens.next() != Some("root") {
                continue;
            }
            return tokens.next();
        }
    }
    None
}

fn restore_key_binding(server: &TmuxServer, previous: Option<&str>) -> Result<()> {
    match previous {
        Some(previous) => {
            let mut child = server
                .command()
                .args(["source-file", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(previous.as_bytes())?;
                stdin.write_all(b"\n")?;
            }
            let output = child.wait_with_output()?;
            command_unit(output, "tmux restore key binding")
        }
        None => {
            let output = server
                .command()
                .args(["unbind-key", "-T", "root", "C-q"])
                .output()?;
            command_unit(output, "tmux unbind-key")
        }
    }
}

pub(super) fn wait_for_return_key(server: &TmuxServer, session: &str) -> Result<()> {
    let signal = return_signal_name(session);
    let mut child = server.command().args(["wait-for", &signal]).spawn()?;
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            return Err(Error::Command {
                context: "tmux wait-for",
                stderr: format!("tmux exited with {status}"),
            });
        }
        if !has_session(server, session)? {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn return_signal_name(session: &str) -> String {
    // Must match the signal emitted by the C-q root binding installed in
    // `ReturnKeyBinding::install` (`robco-return-#{session_name}`). The session
    // name robco creates is already sanitized, so this round-trips to the same
    // string tmux expands `#{session_name}` to.
    format!("robco-return-{}", sanitize_target_part(session))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_binding_key_extracts_key_column() {
        let line = "bind-key  -T root C-q                       run-shell \"tmux switch-client -t '#{@robco_return}' || tmux switch-client -l\"";
        assert_eq!(root_binding_key(line), Some("C-q"));
    }

    #[test]
    fn root_binding_key_ignores_t_root_inside_command_body() {
        // A `-T root` appearing inside the bound command must not be mistaken
        // for the key column; the first `-T root` (the real table) wins.
        let line = "bind-key -T root C-q run-shell \"tmux bind-key -T root x foo\"";
        assert_eq!(root_binding_key(line), Some("C-q"));
    }

    #[test]
    fn root_binding_key_handles_flags_and_non_root() {
        assert_eq!(
            root_binding_key("bind-key -r -T root M-Up resize-pane"),
            Some("M-Up")
        );
        assert_eq!(
            root_binding_key("bind-key -T prefix z resize-pane -Z"),
            None
        );
    }

    #[test]
    fn return_signal_name_is_tmux_safe() {
        let signal = return_signal_name("repo/foo.bar:baz");
        assert!(signal.starts_with("robco-return-"));
        assert!(signal.ends_with("-repo-foo-bar-baz"));
    }
}
