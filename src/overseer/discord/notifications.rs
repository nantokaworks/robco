use super::humanize;
use crate::overseer::{
    config::{DiscordConfig, NotifyTier},
    logging::{DecisionEntry, DecisionKind},
};

/// Discord caps an embed description at 4096 chars.
const DESCRIPTION_LIMIT: usize = 4096;
/// Discord caps an embed field value at 1024 chars.
const FIELD_VALUE_LIMIT: usize = 1024;
/// How many digest alerts are listed before the `… and N more` tail.
const DIGEST_ALERT_LINES: usize = 10;
/// Room reserved inside `DESCRIPTION_LIMIT` for the `… and N more` tail.
const TAIL_RESERVE: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationField {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub description: String,
    pub color: u32,
    /// Inline embed fields (Task / Repo / PR / Reason). Never localized: they
    /// carry ids, links, and raw reason codes that must survive translation
    /// untouched — see `localize.rs`, which translates only title and
    /// description and re-attaches these from the original notification.
    pub fields: Vec<NotificationField>,
}

pub fn from_decision(config: &DiscordConfig, entry: &DecisionEntry) -> Option<Notification> {
    if entry.source.as_deref() == Some("discord") {
        return None;
    }
    // A merge-gate escalation the recheck loop may still resolve on its
    // own — `daemon::merge_escalation` already decided this one stays
    // quiet until it is stuck long enough to notify on its own account.
    if entry.escalation_notify == Some(false) {
        return None;
    }
    let event = if entry.source.as_deref() == Some("daemon_event") {
        Some(entry.reason.as_str())
    } else {
        None
    };
    let (tier, title, color) = match (event, entry.kind) {
        (Some("task_started"), _) => (NotifyTier::Summary, "Task started", 0x95a5a6),
        (Some("pr_opened"), _) => (NotifyTier::All, "PR opened", 0x3498db),
        (Some("merged"), _) => (NotifyTier::Summary, "Merged", 0x2ecc71),
        (Some("task_failed"), _) => (NotifyTier::Errors, "Task failed", 0xc0392b),
        (Some("task_escalated"), _) => (NotifyTier::Errors, "Task escalated", 0xd35400),
        (Some("worker_blocked"), _) => (NotifyTier::Errors, "Worker blocked", 0xe67e22),
        (Some("queue_drained"), _) => (NotifyTier::Summary, "Queue drained", 0x1abc9c),
        (_, DecisionKind::CircuitOpen) => (NotifyTier::Errors, "Circuit open", 0xe74c3c),
        (_, DecisionKind::Escalate) => (NotifyTier::Errors, "Escalation", 0xf1c40f),
        _ => return None,
    };
    if !config.notify_level.admits(tier) {
        return None;
    }
    let mut fields = Vec::new();
    if let Some(task) = &entry.task {
        fields.push(field("Task", format!("`{task}`")));
    }
    if let Some(repo) = &entry.repo {
        fields.push(field("Repo", format!("`{repo}`")));
    }
    if let Some(url) = &entry.pr_url {
        fields.push(field("PR", pr_link(url)));
    }
    // A known code-shaped reason reads as a sentence, and the raw code moves
    // into a `Reason` field — fields are never localized (`localize.rs`), so
    // the code survives translation verbatim. Unknown reasons keep the old
    // behavior: the raw decision text is the description.
    let description = match humanize::sentence(&entry.reason) {
        Some(sentence) => {
            fields.push(field("Reason", format!("`{}`", entry.reason)));
            clip(&sentence, DESCRIPTION_LIMIT)
        }
        None => clip(&entry.reason, DESCRIPTION_LIMIT),
    };
    Some(Notification {
        title: title.into(),
        description,
        color,
        fields,
    })
}

fn field(name: &str, value: String) -> NotificationField {
    NotificationField {
        name: name.into(),
        value: clip(&value, FIELD_VALUE_LIMIT),
    }
}

/// A clickable markdown link for a PR url, labeled `#123` when the url has
/// the usual `/pull/123` shape, or the raw url otherwise.
fn pr_link(url: &str) -> String {
    let label = url
        .rsplit_once("/pull/")
        .map(|(_, number)| format!("#{number}"))
        .unwrap_or_else(|| url.to_string());
    format!("[{label}]({url})")
}

fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.into();
    }
    let mut clipped: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}

pub fn digest(config: &DiscordConfig, entries: &[DecisionEntry]) -> Option<Notification> {
    let enabled = entries
        .iter()
        .filter(|entry| entry.source.as_deref() != Some("discord"))
        // Same rule `from_decision` applies per-decision: a merge escalation
        // the recheck loop may still resolve on its own must not re-surface
        // here just because it rode into the digest alongside other alerts.
        .filter(|entry| entry.escalation_notify != Some(false))
        .filter(|entry| match entry.kind {
            DecisionKind::CircuitOpen | DecisionKind::Escalate => {
                config.notify_level.admits(NotifyTier::Errors)
            }
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    if enabled.is_empty() {
        return None;
    }
    Some(Notification {
        title: "Overseer digest".into(),
        description: digest_description(&enabled),
        color: 0xf1c40f,
        fields: Vec::new(),
    })
}

/// One markdown bullet per alert, bounded by both a line count and the embed
/// description limit; alerts that do not fit collapse into `… and N more`.
fn digest_description(alerts: &[DecisionEntry]) -> String {
    let mut lines = vec![format!("**{} overseer alert(s)**", alerts.len())];
    let mut used = lines[0].chars().count();
    let mut shown = 0;
    for entry in alerts.iter().take(DIGEST_ALERT_LINES) {
        let target = entry.task.as_deref().unwrap_or("overseer");
        let line = format!("- `{target}`: {}", clip(&entry.reason, 200));
        if used + line.chars().count() + 1 > DESCRIPTION_LIMIT - TAIL_RESERVE {
            break;
        }
        used += line.chars().count() + 1;
        lines.push(line);
        shown += 1;
    }
    let remaining = alerts.len() - shown;
    if remaining > 0 {
        lines.push(format!("… and {remaining} more"));
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;
