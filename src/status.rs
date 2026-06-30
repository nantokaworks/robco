use chrono::{Duration, Local};

use crate::{agent, model::Status, tmux};

pub fn refresh_agent(agent: &mut crate::model::AgentNode, auto_accept: bool) {
    if agent::ensure_agent_session(agent).is_err() {
        agent.status = Status::Dead;
        return;
    }

    let capture = tmux::capture_text(&agent.tmux_session).unwrap_or_default();
    let signature = status_signature(&capture);
    let now = Local::now();
    let changed = agent
        .last_capture
        .as_ref()
        .map(|last| last != &signature)
        .unwrap_or(false);

    if changed {
        agent.last_change_at = Some(now);
    }
    agent.last_capture = Some(signature);

    if looks_waiting(&capture) {
        agent.status = Status::Waiting;
        maybe_auto_accept(agent, auto_accept, now);
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
