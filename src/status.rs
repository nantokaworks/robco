use chrono::{Duration, Local};

use std::path::Path;

use crate::{git, model::Status, tmux};

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

    if looks_waiting(capture) {
        Status::Waiting
    } else if state
        .last_change_at
        .map(|changed_at| now - changed_at < Duration::seconds(3))
        .unwrap_or(false)
    {
        Status::Running
    } else {
        Status::Idle
    }
}

pub fn looks_waiting(capture: &str) -> bool {
    let lower = capture.to_ascii_lowercase();
    lower.contains("allow") && lower.contains("(y/n)")
        || lower.contains("do you want to")
        || lower.contains("continue?")
        || lower.lines().last().is_some_and(|line| {
            let trimmed = line.trim();
            trimmed.ends_with('>') || trimmed.ends_with('?')
        })
}

fn status_signature(capture: &str) -> String {
    capture
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
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
mod tests {
    use super::*;

    #[test]
    fn detects_common_confirmation_prompts() {
        assert!(looks_waiting("Allow edit src/main.rs? (y/n)"));
        assert!(looks_waiting("Do you want to continue?"));
        assert!(!looks_waiting("running cargo test"));
    }

    #[test]
    fn status_signature_ignores_trailing_whitespace() {
        assert_eq!(
            status_signature("hello   \nworld\t\n\n\n"),
            status_signature("hello\nworld")
        );
    }
}
