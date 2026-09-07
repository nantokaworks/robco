use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use crate::{
    Result,
    locale::{Locale, fmt},
    model::HostLabel,
};

/// Hard cap on the probe's TOTAL runtime. `ConnectTimeout` only bounds the
/// TCP connect — DNS, proxies, or a remote command that stalls after the
/// connection are outside it, and the probe runs while the TUI is suspended,
/// so an unbounded hang would freeze the whole screen.
const PROBE_CAP: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ProbeVerdict {
    Exists,
    MissingSession,
    TmuxNotFound,
    Failed(String),
}

pub(super) fn classify_probe(exit_ok: bool, stderr: &str) -> ProbeVerdict {
    if exit_ok {
        return ProbeVerdict::Exists;
    }
    let lower = stderr.to_ascii_lowercase();
    // `error connecting` alone is not enough: tmux emits it for permission
    // and other socket errors too. Only the no-socket variant means "no
    // server, so no session"; the rest surface as real failures.
    if lower.contains("can't find session")
        || lower.contains("no server running")
        || (lower.contains("error connecting") && lower.contains("no such file"))
    {
        ProbeVerdict::MissingSession
    } else if lower.contains("tmux")
        && (lower.contains("command not found") || lower.contains("not found"))
    {
        ProbeVerdict::TmuxNotFound
    } else {
        ProbeVerdict::Failed(
            stderr
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(str::trim)
                .unwrap_or_default()
                .to_string(),
        )
    }
}

pub(super) fn probe(host: &HostLabel, session: &str) -> Result<ProbeVerdict> {
    let target = format!("={session}");
    let mut command = Command::new("ssh");
    command
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            &host.ssh,
            "--",
            "tmux",
            "has-session",
            "-t",
            &target,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let deadline = Instant::now() + PROBE_CAP;
    let timed_out = loop {
        if child.try_wait()?.is_some() {
            break false;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break true;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = child.wait_with_output()?;
    if timed_out {
        return Ok(ProbeVerdict::Failed(format!(
            "probe timed out after {}s",
            PROBE_CAP.as_secs()
        )));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let verdict = classify_probe(output.status.success(), &stderr);
    Ok(match verdict {
        ProbeVerdict::Failed(detail) if detail.is_empty() => {
            ProbeVerdict::Failed(format!("ssh exited with {}", output.status))
        }
        verdict => verdict,
    })
}

/// Single-quote `value` for the copy-pasteable hint: session names and ssh
/// destinations come from adopted/remote state and an unquoted metacharacter
/// would change the hint's shell meaning.
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

pub(super) fn attach_with(
    probe: impl FnOnce() -> Result<ProbeVerdict>,
    attach: impl FnOnce() -> Result<()>,
) -> Result<ProbeVerdict> {
    let verdict = probe()?;
    if verdict == ProbeVerdict::Exists {
        attach()?;
    }
    Ok(verdict)
}

/// Direct ssh attach intentionally nests the operator's local and remote tmux.
pub(super) fn interactive(host: &HostLabel, session: &str) -> Result<()> {
    let target = format!("={session}");
    let status = Command::new("ssh")
        .args(["-t", &host.ssh, "tmux", "attach", "-t", &target])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(crate::Error::Command {
            context: "remote tmux attach",
            stderr: format!(
                "ssh exited with {status}; re-run manually: ssh -t {} tmux attach -t {}",
                sh_quote(&host.ssh),
                sh_quote(&target)
            ),
        })
    }
}

impl ProbeVerdict {
    pub(super) fn message(&self, locale: Locale, host: &HostLabel, session: &str) -> String {
        match self {
            Self::Exists => String::new(),
            Self::MissingSession => fmt(
                locale,
                "no live session {} on {} — start it on the host first",
                &[session, &host.name],
            ),
            Self::TmuxNotFound => fmt(
                locale,
                "tmux is not on {}'s non-interactive PATH",
                &[&host.name],
            ),
            Self::Failed(detail) => fmt(locale, "remote attach failed: {}", &[detail]),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn classifies_probe_results() {
        assert_eq!(classify_probe(true, ""), ProbeVerdict::Exists);
        assert_eq!(
            classify_probe(false, "can't find session: work"),
            ProbeVerdict::MissingSession
        );
        assert_eq!(
            classify_probe(false, "no server running on /tmp/tmux-501/default"),
            ProbeVerdict::MissingSession
        );
        assert_eq!(
            classify_probe(
                false,
                "error connecting to /tmp/tmux-501/default (No such file or directory)"
            ),
            ProbeVerdict::MissingSession
        );
        // The same prefix with a permission error is a real failure, not a
        // missing session — advising "start it on the host" would mislead.
        assert_eq!(
            classify_probe(
                false,
                "error connecting to /tmp/tmux-501/default (Permission denied)"
            ),
            ProbeVerdict::Failed(
                "error connecting to /tmp/tmux-501/default (Permission denied)".into()
            )
        );
        assert_eq!(
            classify_probe(false, "sh: tmux: command not found"),
            ProbeVerdict::TmuxNotFound
        );
        assert_eq!(
            classify_probe(false, "Permission denied (publickey).\nignored"),
            ProbeVerdict::Failed("Permission denied (publickey).".into())
        );
        assert_eq!(
            classify_probe(false, ""),
            ProbeVerdict::Failed(String::new())
        );
    }

    #[test]
    fn missing_session_does_not_invoke_interactive_attach() {
        let attached = Cell::new(false);
        let verdict = attach_with(
            || Ok(ProbeVerdict::MissingSession),
            || {
                attached.set(true);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(verdict, ProbeVerdict::MissingSession);
        assert!(!attached.get());
    }

    #[test]
    fn the_manual_hint_quotes_shell_metacharacters() {
        assert_eq!(sh_quote("robco_x_main"), "'robco_x_main'");
        assert_eq!(sh_quote("a b'c"), r"'a b'\''c'");
    }
}
