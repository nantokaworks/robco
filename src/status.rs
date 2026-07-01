use chrono::{Duration, Local};

use std::path::Path;

use crate::{git, model::Status, tmux};

const WORKING_MARKER: &str = "esc to interrupt";
const STRONG_WAITING_MARKERS: &[&str] = &["(y/n)", "(y/N)", "do you want to"];
const SPINNER_PIPS: &[char] = &['✢', '✣', '✤', '✥', '✦', '✧', '✳', '✽', '●', '○'];
const CURSOR_BLOCKS: &[char] = &['█', '▌', '▍', '▎', '▏', '▐', '▕'];
const BOX_CHARS: &str = "│┃║╎┌┐└┘─━═╭╮╰╯├┤┬┴┼";

#[derive(Debug, Default)]
pub struct WatchStatusState {
    pub last_capture: Option<String>,
    pub last_change_at: Option<chrono::DateTime<Local>>,
}

pub fn refresh_agent(repo_path: &Path, agent: &mut crate::model::AgentNode, auto_accept: bool) {
    let mut state = WatchStatusState {
        last_capture: agent.last_capture.take(),
        last_change_at: agent.last_change_at.take(),
    };

    if let Some(status) = classify_agent_status(
        repo_path,
        &agent.worktree_path,
        &agent.branch,
        &agent.tmux_session,
        &mut state,
    ) {
        agent.status = status;
        if status == Status::Waiting {
            maybe_auto_accept(agent, auto_accept, Local::now());
        }
    }

    agent.last_capture = state.last_capture;
    agent.last_change_at = state.last_change_at;
}

pub fn classify_agent_status(
    repo_path: &Path,
    worktree_path: &Path,
    branch: &str,
    tmux_session: &str,
    state: &mut WatchStatusState,
) -> Option<Status> {
    if !worktree_path.exists() {
        return Some(if git::branch_exists(repo_path, branch).unwrap_or(false) {
            Status::BranchOnly
        } else {
            Status::Dead
        });
    }

    // A status refresh only *observes* the tmux session; it must never create
    // one. Distinguish "the session is gone" from "couldn't probe it": a
    // transient failure to run `tmux` (e.g. a fork/exec hiccup under load) makes
    // `has_session` return `Err`, and treating that as death is what flipped a
    // healthy, running agent to `dead`. Keep the previous status and retry on
    // the next tick instead.
    match tmux::has_session(tmux_session) {
        Ok(true) => {}
        Ok(false) => return Some(Status::Dead),
        Err(_) => return None,
    }

    // Likewise, a transient capture failure should not corrupt the Running/Idle
    // signal; keep the previous status until the next successful capture.
    let Ok(capture) = tmux::capture_text(tmux_session) else {
        return None;
    };
    Some(classify_capture(&capture, state, Local::now()))
}

fn classify_capture(
    capture: &str,
    state: &mut WatchStatusState,
    now: chrono::DateTime<Local>,
) -> Status {
    let waiting = looks_waiting(capture);
    let working = looks_working(capture);
    let signature = status_signature(capture);
    let changed = state
        .last_capture
        .as_ref()
        .map(|last| last != &signature)
        .unwrap_or(false);

    if changed {
        state.last_change_at = Some(now);
    }
    state.last_capture = Some(signature);

    let recently_changed = state
        .last_change_at
        .map(|changed_at| now - changed_at < Duration::seconds(3))
        .unwrap_or(false);

    if waiting {
        Status::Waiting
    } else if working || recently_changed {
        Status::Running
    } else {
        Status::Idle
    }
}

pub fn looks_waiting(capture: &str) -> bool {
    looks_strong_waiting(capture) || (!looks_working(capture) && looks_weak_waiting(capture))
}

fn looks_strong_waiting(capture: &str) -> bool {
    capture.lines().any(|line| {
        let trimmed = trim_line_chrome(line);
        let lower = trimmed.to_ascii_lowercase();
        STRONG_WAITING_MARKERS
            .iter()
            .any(|marker| lower.contains(&marker.to_ascii_lowercase()))
            || looks_option_line(trimmed)
    })
}

fn looks_working(capture: &str) -> bool {
    capture
        .to_ascii_lowercase()
        .contains(&WORKING_MARKER.to_ascii_lowercase())
}

fn looks_weak_waiting(capture: &str) -> bool {
    capture
        .lines()
        .rev()
        .map(trim_line_chrome)
        .find(|line| !line.is_empty() && !is_border_line(line) && !is_footer_line(line))
        .is_some_and(|line| line.ends_with('?') || line.ends_with('>'))
}

fn looks_option_line(line: &str) -> bool {
    let mut line = line.trim_start();
    let had_arrow = line.starts_with(['❯', '>']);
    if had_arrow {
        line = line.trim_start_matches(['❯', '>']).trim_start();
    }
    let Some(line) = line
        .strip_prefix(|c: char| c.is_ascii_digit())
        .and_then(|rest| rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')))
        .map(str::trim_start)
        .or(had_arrow.then_some(line))
    else {
        return false;
    };
    matches!(
        line.to_ascii_lowercase().split_whitespace().next(),
        Some("yes" | "no")
    )
}

fn status_signature(capture: &str) -> String {
    capture
        .lines()
        .filter_map(normalize_signature_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_signature_line(line: &str) -> Option<String> {
    let line = trim_line_chrome(line);
    if line.is_empty() || is_footer_line(line) || is_border_line(line) {
        return None;
    }

    let without_spinners: String = line.chars().filter(|ch| !is_spinner_char(*ch)).collect();
    let collapsed = without_spinners
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}

fn trim_line_chrome(line: &str) -> &str {
    line.trim()
        .trim_matches(is_box_char)
        .trim()
        .trim_matches(|ch| CURSOR_BLOCKS.contains(&ch))
        .trim()
}

fn is_box_char(ch: char) -> bool {
    BOX_CHARS.contains(ch)
}

fn is_border_line(line: &str) -> bool {
    !line.is_empty()
        && line
            .chars()
            .all(|ch| ch.is_whitespace() || is_box_char(ch) || CURSOR_BLOCKS.contains(&ch))
}

fn is_footer_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("for shortcuts")
        || (lower.contains("model") && lower.contains("token"))
        || lower.contains("elapsed")
        || has_token_counter(&lower)
}

fn has_token_counter(line: &str) -> bool {
    line.match_indices("token").any(|(idx, token)| {
        let rest = &line[idx + token.len()..];
        let before = line[..idx]
            .rsplit(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("");
        let after = rest.strip_prefix('s').unwrap_or(rest);
        let after = after
            .split(|c: char| c.is_alphabetic())
            .next()
            .unwrap_or("");
        [before, after]
            .iter()
            .any(|part| part.contains(|c: char| c.is_ascii_digit()))
    })
}

fn is_spinner_char(ch: char) -> bool {
    ('\u{2800}'..='\u{28ff}').contains(&ch) || SPINNER_PIPS.contains(&ch)
}

fn maybe_auto_accept(
    agent: &mut crate::model::AgentNode,
    auto_accept: bool,
    now: chrono::DateTime<Local>,
) {
    if !auto_accept {
        return;
    }

    let recently_sent = agent
        .last_auto_accept_at
        .map(|sent_at| now - sent_at < Duration::seconds(5))
        .unwrap_or(false);
    if recently_sent {
        return;
    }

    if tmux::send_keys(&agent.tmux_session, &["y", "Enter"]).is_ok() {
        agent.last_auto_accept_at = Some(now);
    }
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed_now() -> chrono::DateTime<Local> {
        Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
    }

    #[test]
    fn detects_common_confirmation_prompts() {
        assert!(looks_waiting("Allow edit src/main.rs? (y/n)"));
        assert!(looks_waiting("Do you want to continue?"));
        assert!(looks_waiting("  │ ❯ 1. Yes │\n  │   2. No  │"));
        assert!(looks_waiting("Enter API token?"));
        assert!(!looks_waiting("No tests failed"));
        assert!(!looks_waiting("Yes, the change is complete"));
        assert!(!looks_waiting("running cargo test"));
    }

    #[test]
    fn stopped_with_live_chrome_goes_idle() {
        let mut state = WatchStatusState::default();
        let first = "Done\n  Tokens: 1 █\n  ? for shortcuts";
        let second = "Done\n  Tokens: 2 ▌\n  ? for shortcuts";
        assert_eq!(classify_capture(first, &mut state, fixed_now()), Status::Idle);
        assert_eq!(classify_capture(second, &mut state, fixed_now() + Duration::seconds(1)), Status::Idle);
    }

    #[test]
    fn boxed_permission_prompt_waits_despite_footer() {
        let mut state = WatchStatusState::default();
        let capture = "╭────╮\n│ Do you want to proceed? │\n│ ❯ 1. Yes │\n│   2. No │\n╰────╯\n  ? for shortcuts";
        assert_eq!(classify_capture(capture, &mut state, fixed_now()), Status::Waiting);
    }

    #[test]
    fn working_marker_forces_running() {
        let mut state = WatchStatusState::default();
        assert_eq!(classify_capture("Generating response (esc to interrupt)", &mut state, fixed_now()), Status::Running);
    }

    #[test]
    fn status_signature_ignores_trailing_whitespace() {
        assert_eq!(status_signature("hello   \nworld\t\n\n\n"), status_signature("hello\nworld"));
    }
}
