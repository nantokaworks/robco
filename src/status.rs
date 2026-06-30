use chrono::{Duration, Local};

use crate::{model::Status, tmux};

pub fn refresh_agent(agent: &mut crate::model::AgentNode) {
    if !tmux::has_session(&agent.tmux_session).unwrap_or(false) {
        agent.status = Status::Dead;
        return;
    }

    let capture = tmux::capture_plain(&agent.tmux_session).unwrap_or_default();
    let now = Local::now();
    let changed = agent
        .last_capture
        .as_ref()
        .map(|last| last != &capture)
        .unwrap_or(false);

    if changed {
        agent.last_change_at = Some(now);
    }
    agent.last_capture = Some(capture.clone());

    if looks_waiting(&capture) {
        agent.status = Status::Waiting;
    } else if agent
        .last_change_at
        .map(|changed_at| now - changed_at < Duration::seconds(3))
        .unwrap_or(false)
    {
        agent.status = Status::Running;
    } else {
        agent.status = Status::Idle;
    }
}

fn looks_waiting(capture: &str) -> bool {
    let lower = capture.to_ascii_lowercase();
    lower.contains("allow") && lower.contains("(y/n)")
        || lower.contains("do you want to")
        || lower.contains("continue?")
        || lower.lines().last().is_some_and(|line| {
            let trimmed = line.trim();
            trimmed.ends_with('>') || trimmed.ends_with('?')
        })
}
