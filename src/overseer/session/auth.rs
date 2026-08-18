//! Telling an authentication failure apart from every other reason a session
//! produced no `result.json`.
//!
//! Without this the daemon sees only "the process exited and wrote nothing",
//! which is the same shape as a model that ran out of turns or a briefing it
//! refused — and a triage or review session that reports the generic shape
//! fails silently without ever naming the credential as the cause. The agent
//! CLIs do say so on stderr, so the session captures stderr to `session.log`
//! and this module reads the tail of it.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

/// The reason every surface uses when the credential, not the work, is what
/// failed. One string so a decision log, a Discord reply, and an inbox row can
/// all be grepped for the same cause — and so an operator reading
/// `decisions.jsonl` sees something other than the generic fail-safe wording
/// that says nothing about credentials.
pub(crate) const REASON: &str = "session_auth_failed";

/// Bounded read of `session.log`: a session that fails to authenticate says so
/// in its first few lines, and an unbounded read would let a chatty process
/// pull its whole output into memory and into a decision reason.
const LOG_TAIL_BYTES: u64 = 8 * 1024;

/// The last [`LOG_TAIL_BYTES`] of a session log, as lossy UTF-8. Every read
/// failure is an empty log rather than an error: the log exists to explain a
/// failure that has already happened, so failing to read it must not become a
/// second failure.
pub(crate) fn read_log_tail(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let Ok(length) = file.metadata().map(|metadata| metadata.len()) else {
        return String::new();
    };
    if file
        .seek(SeekFrom::Start(length.saturating_sub(LOG_TAIL_BYTES)))
        .is_err()
    {
        return String::new();
    }
    let mut raw = Vec::new();
    match file.take(LOG_TAIL_BYTES).read_to_end(&mut raw) {
        Ok(_) => String::from_utf8_lossy(&raw).into_owned(),
        Err(_) => String::new(),
    }
}

/// Phrases the agent CLIs use when the credential, not the work, is what
/// failed. Kept narrow on purpose: a false positive would blame authentication
/// for a failure an operator then cannot find in their credential channel.
const SIGNATURES: [&str; 8] = [
    "oauth session expired",
    "failed to authenticate",
    "authentication_failed",
    "authentication failed",
    "invalid api key",
    "invalid bearer token",
    "not logged in",
    "please run `claude login`",
];

/// The whole classification, over a session log that may not exist: `Some`
/// carries the line to report, `None` means this failure was something else.
pub(crate) fn failure_detail(log_path: &Path) -> Option<String> {
    let log = read_log_tail(log_path);
    is_auth_failure(&log).then(|| summarize(&log))
}

fn is_auth_failure(log: &str) -> bool {
    let log = log.to_ascii_lowercase();
    SIGNATURES.iter().any(|signature| log.contains(signature))
}

/// The one line worth putting in a decision reason: the first that carries a
/// signature, trimmed of terminal control bytes and capped so a decision entry
/// stays one readable line.
fn summarize(log: &str) -> String {
    const MAX: usize = 200;
    let line = log
        .lines()
        .map(sanitize)
        .find(|line| is_auth_failure(line))
        .unwrap_or_default();
    if line.chars().count() <= MAX {
        return line;
    }
    let truncated = line.chars().take(MAX).collect::<String>();
    format!("{truncated}…")
}

/// Strip ANSI escape sequences and other control characters. The agent CLIs
/// colour their errors, and a raw escape byte in a JSONL decision reason makes
/// the log unreadable in exactly the situation an operator is reading it.
fn sanitize(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            for escaped in chars.by_ref() {
                if escaped.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if character.is_control() {
            out.push(' ');
        } else {
            out.push(character);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_observed_oauth_failure() {
        let log = "\u{1b}[31mFailed to authenticate: OAuth session expired and could not be refreshed\u{1b}[0m\n";

        assert!(is_auth_failure(log));
        assert_eq!(
            summarize(log),
            "Failed to authenticate: OAuth session expired and could not be refreshed"
        );
    }

    #[test]
    fn ordinary_session_noise_is_not_an_auth_failure() {
        assert!(!is_auth_failure("rate limit exceeded, retrying\n"));
        assert!(!is_auth_failure("Error: token budget exhausted\n"));
        assert!(!is_auth_failure(""));
        assert_eq!(summarize("rate limit exceeded\n"), "");
    }

    #[test]
    fn summary_picks_the_matching_line_out_of_surrounding_output() {
        let log = "starting\nreading briefing.md\nInvalid API key · Please run /login\ndone\n";

        assert_eq!(summarize(log), "Invalid API key · Please run /login");
    }

    #[test]
    fn summary_is_capped_to_one_readable_line() {
        let log = format!("not logged in {}", "x".repeat(500));

        let summary = summarize(&log);
        assert_eq!(summary.chars().count(), 201);
        assert!(summary.ends_with('…'));
    }

    #[test]
    fn log_tail_is_bounded_and_absent_logs_read_empty() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.log");

        assert_eq!(read_log_tail(&path), "");

        std::fs::write(&path, "a".repeat(LOG_TAIL_BYTES as usize + 64)).unwrap();
        assert_eq!(read_log_tail(&path).len(), LOG_TAIL_BYTES as usize);
    }

    #[test]
    fn failure_detail_reads_a_session_log_and_names_only_auth_failures() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.log");

        // A session that never started leaves no log at all.
        assert_eq!(failure_detail(&path), None);

        std::fs::write(&path, "compacting context\nrate limited, retrying\n").unwrap();
        assert_eq!(failure_detail(&path), None);

        std::fs::write(
            &path,
            "Failed to authenticate: OAuth session expired and could not be refreshed\n",
        )
        .unwrap();
        assert_eq!(
            failure_detail(&path).as_deref(),
            Some("Failed to authenticate: OAuth session expired and could not be refreshed")
        );
    }
}
