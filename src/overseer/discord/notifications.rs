use crate::overseer::{
    config::DiscordConfig,
    logging::{DecisionEntry, DecisionKind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub description: String,
    pub color: u32,
}

pub fn from_decision(config: &DiscordConfig, entry: &DecisionEntry) -> Option<Notification> {
    if entry.source.as_deref() == Some("discord") {
        return None;
    }
    let event = if entry.source.as_deref() == Some("daemon_event") {
        Some(entry.reason.as_str())
    } else {
        None
    };
    let (enabled, title, color) = match (event, entry.kind) {
        (Some("pr_opened"), _) => (config.notify_pr_opened, "PR opened", 0x3498db),
        (Some("merged"), _) => (config.notify_merged, "Merged", 0x2ecc71),
        (Some("worker_blocked"), _) => (config.notify_worker_blocked, "Worker blocked", 0xe67e22),
        (_, DecisionKind::CircuitOpen) => (config.notify_circuit, "Circuit open", 0xe74c3c),
        (_, DecisionKind::Escalate) => (config.notify_escalation, "Escalation", 0xf1c40f),
        _ => return None,
    };
    if !enabled {
        return None;
    }
    let mut details = vec![entry.reason.clone()];
    if let Some(task) = &entry.task {
        details.push(format!("Task: {task}"));
    }
    if let Some(repo) = &entry.repo {
        details.push(format!("Repo: {repo}"));
    }
    if let Some(url) = &entry.pr_url {
        details.push(format!("PR: {url}"));
    }
    Some(Notification {
        title: title.into(),
        description: details.join("\n"),
        color,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_includes_task_and_pr_url() {
        let mut entry = DecisionEntry::new(DecisionKind::Hold, "pr_opened");
        entry.source = Some("daemon_event".into());
        entry.task = Some("task-135".into());
        entry.pr_url = Some("https://example.test/pull/1".into());
        let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
        assert!(notification.description.contains("task-135"));
        assert!(
            notification
                .description
                .contains("https://example.test/pull/1")
        );
    }
}
