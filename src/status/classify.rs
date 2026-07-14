use chrono::{Duration, Local};

use crate::model::Status;

use super::{StatusReport, WatchStatusState};

const WORKING_MARKER: &str = "esc to interrupt";
/// Markers of a *real* confirmation / selection prompt — the only signal that
/// may drive auto-accept. `no, and tell claude what to do differently` is the
/// last option of Claude Code's tool-permission dialog (the same anchor
/// claude-squad keys on); it is far more specific than the ever-present `❯`
/// input caret.
const STRONG_WAITING_MARKERS: &[&str] = &[
    "(y/n)",
    "(y/N)",
    "do you want to",
    "no, and tell claude what to do differently",
];
const SPINNER_PIPS: &[char] = &['✢', '✣', '✤', '✥', '✦', '✧', '✳', '✽', '●', '○'];
const CURSOR_BLOCKS: &[char] = &['█', '▌', '▍', '▎', '▏', '▐', '▕'];
const BOX_CHARS: &str = "│┃║╎┌┐└┘─━═╭╮╰╯├┤┬┴┼";

/// Classify a single tmux capture into a display [`Status`] plus whether the
/// screen shows a real confirmation prompt.
///
/// The four AI-session outcomes:
/// - `Running`  — `esc to interrupt` is on screen (actively working).
/// - `Waiting`  — a real confirmation / selection prompt (`awaiting_confirmation`).
/// - `Done`     — finished a turn and sitting at the `❯` input caret, settled.
/// - `Idle`     — session alive but nothing pending and no input caret.
///
/// Only `awaiting_confirmation` (a real prompt) gates auto-accept, so a plain
/// input prompt or a finished turn never gets a spurious `y`+Enter typed in.
pub(super) fn classify_capture(
    capture: &str,
    state: &mut WatchStatusState,
    now: chrono::DateTime<Local>,
) -> StatusReport {
    let working = looks_working(capture);
    // A real confirmation prompt (y/n / option list). This is the only signal
    // that may drive auto-accept.
    let confirmation = looks_strong_waiting(capture);
    // Claude finished a turn and is sitting at its input caret (`❯`). Present
    // whenever Claude is at rest — so it must NOT alone mean "waiting", only
    // "done" once the screen settles.
    let at_prompt = looks_input_prompt(capture);
    // Generic weak waiting (a line ending in `?`/`>`), kept only as a fallback
    // for programs that do NOT render the `❯` caret (non-Claude tools). It must
    // never override Claude's `Done` state — otherwise a finished turn whose
    // last line is a prose question would flip to `wait`.
    let weak = !working && !at_prompt && looks_weak_waiting(capture);

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

    let status = if confirmation || weak {
        Status::Waiting
    } else if working || recently_changed {
        Status::Running
    } else if at_prompt {
        Status::Done
    } else {
        Status::Idle
    };

    StatusReport {
        status,
        awaiting_confirmation: confirmation,
        worktree_missing: false,
    }
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

/// Whether the capture shows Claude's input prompt caret (`❯`). Present both
/// while working and at rest, so callers gate on `!looks_working` (working) and
/// on the settle buffer (recently changed) before treating it as `Done`.
fn looks_input_prompt(capture: &str) -> bool {
    capture
        .lines()
        .any(|line| trim_line_chrome(line).starts_with('❯'))
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
        || is_mode_bar(&lower)
        || has_token_counter(&lower)
}

/// Claude Code's persistent bottom mode bar, e.g.
/// `⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt · ← for agents`.
/// It is chrome, not content: without skipping it, `looks_weak_waiting` would
/// always inspect this bar instead of the real prompt/question line above it.
/// `looks_working` still sees `esc to interrupt` because it scans the raw
/// capture, independent of this footer filtering.
fn is_mode_bar(lower: &str) -> bool {
    lower.contains("shift+tab to cycle")
        || lower.contains("bypass permissions")
        || lower.contains("accept edits")
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

#[cfg(test)]
fn looks_waiting(capture: &str) -> bool {
    looks_strong_waiting(capture)
        || (!looks_working(capture) && !looks_input_prompt(capture) && looks_weak_waiting(capture))
}

#[cfg(test)]
mod tests;
