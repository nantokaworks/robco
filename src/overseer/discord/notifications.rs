use super::humanize;
use crate::overseer::{
    config::{DiscordConfig, NotifyTier},
    logging::{DecisionEntry, DecisionKind},
};
use std::collections::HashMap;

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
    // Tier assignment (dropr:397): `summary` means milestones only — a
    // finished task (`merged`) plus everything in Errors. Step-by-step
    // events (`task_started`, `pr_opened`, `queue_drained`) are `all`-tier:
    // they narrate progress rather than mark a milestone, and at ~10+
    // dispatches/day across repos they turn `summary` into a firehose.
    let (tier, title, color) = match (event, entry.kind) {
        (Some("task_started"), _) => (NotifyTier::All, "Task started", 0x95a5a6),
        (Some("pr_opened"), _) => (NotifyTier::All, "PR opened", 0x3498db),
        (Some("merged"), _) => (NotifyTier::Summary, "Merged", 0x2ecc71),
        (Some("task_failed"), _) => (NotifyTier::Errors, "Task failed", 0xc0392b),
        (Some("task_escalated"), _) => (NotifyTier::Errors, "Task escalated", 0xd35400),
        (Some("worker_blocked"), _) => (NotifyTier::Errors, "Worker blocked", 0xe67e22),
        (Some("queue_drained"), _) => (NotifyTier::All, "Queue drained", 0x1abc9c),
        (Some("advisory_task_filed"), _) => (NotifyTier::Summary, "Advisory task filed", 0xf39c12),
        (Some("dependabot_task_filed"), _) => (
            NotifyTier::Summary,
            "Dependabot coordination task filed",
            0xf39c12,
        ),
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

pub(super) fn field(name: &str, value: String) -> NotificationField {
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

/// The digest-eligible subset of a decision run: the same per-decision rules
/// `from_decision` applies (Discord-sourced entries stay out, a merge
/// escalation the recheck loop may still resolve on its own stays quiet),
/// plus the errors-tier gate for the alert kinds a digest carries.
pub(super) fn digest_alerts(
    config: &DiscordConfig,
    entries: &[DecisionEntry],
) -> Vec<DecisionEntry> {
    entries
        .iter()
        .filter(|entry| entry.source.as_deref() != Some("discord"))
        .filter(|entry| entry.escalation_notify != Some(false))
        .filter(|entry| match entry.kind {
            DecisionKind::CircuitOpen | DecisionKind::Escalate => {
                config.notify_level.admits(NotifyTier::Errors)
            }
            _ => false,
        })
        .cloned()
        .collect()
}

/// Collapses repeats of the same (task, reason) pair — the merge pass
/// re-escalates a held pull request every pass, so one stuck task can ride
/// into a digest several times — into one entry with an occurrence count.
/// First-seen order and first-seen metadata are kept.
pub(super) fn dedup_alerts(alerts: Vec<DecisionEntry>) -> Vec<(DecisionEntry, usize)> {
    let mut deduped: Vec<(DecisionEntry, usize)> = Vec::new();
    for entry in alerts {
        match deduped
            .iter_mut()
            .find(|(seen, _)| seen.task == entry.task && seen.reason == entry.reason)
        {
            Some((_, count)) => *count += 1,
            None => deduped.push((entry, 1)),
        }
    }
    deduped
}

/// The digest embed for an already-filtered, already-deduped alert run.
/// `display_ids` maps task nanoids to dropr `#N` display ids, best-effort.
pub(super) fn digest_notification(
    alerts: &[(DecisionEntry, usize)],
    display_ids: &HashMap<String, String>,
) -> Notification {
    Notification {
        title: "Overseer digest".into(),
        description: digest_description(alerts, display_ids),
        color: 0xf1c40f,
        fields: Vec::new(),
    }
}

/// One markdown bullet per distinct alert, bounded by both a line count and
/// the embed description limit; alerts that do not fit collapse into
/// `… and N more`.
fn digest_description(
    alerts: &[(DecisionEntry, usize)],
    display_ids: &HashMap<String, String>,
) -> String {
    let mut lines = vec![format!("**{} overseer alert(s)**", alerts.len())];
    let mut used = lines[0].chars().count();
    let mut shown = 0;
    for (entry, count) in alerts.iter().take(DIGEST_ALERT_LINES) {
        let line = digest_line(entry, *count, display_ids);
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

/// `- repo · #N [#123](url): sentence ×count` — every part optional except
/// the task label and the reason, which humanizes exactly like a single
/// notification's description so a digest line never reads as a bare code.
fn digest_line(
    entry: &DecisionEntry,
    count: usize,
    display_ids: &HashMap<String, String>,
) -> String {
    let mut line = String::from("- ");
    if let Some(repo) = entry
        .repo
        .as_deref()
        .map(repo_short_name)
        .filter(|repo| !repo.is_empty())
    {
        line.push_str(repo);
        line.push_str(" · ");
    }
    line.push_str(&task_label(entry, display_ids));
    if let Some(url) = &entry.pr_url {
        line.push(' ');
        line.push_str(&pr_link(url));
    }
    line.push_str(": ");
    let sentence = humanize::sentence(&entry.reason).unwrap_or_else(|| entry.reason.clone());
    line.push_str(&clip(&sentence, 200));
    if count > 1 {
        line.push_str(&format!(" ×{count}"));
    }
    line
}

/// The dropr `#N` display id when the ledger still knows this task, the
/// backticked nanoid otherwise, or `overseer` for an alert with no task.
fn task_label(entry: &DecisionEntry, display_ids: &HashMap<String, String>) -> String {
    match &entry.task {
        Some(task) => display_ids
            .get(task)
            .cloned()
            .unwrap_or_else(|| format!("`{task}`")),
        None => "overseer".into(),
    }
}

/// The last path segment: ledger repos are filesystem paths, and the digest
/// only has room for the directory name the operator knows the repo by.
fn repo_short_name(repo: &str) -> &str {
    repo.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(repo)
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;
