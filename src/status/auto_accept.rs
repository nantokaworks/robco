use chrono::{Duration, Local};

use crate::tmux;

pub(super) fn maybe_auto_accept(
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
