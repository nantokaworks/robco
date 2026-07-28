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
        (Some("task_started"), _) => (config.notify_task_started, "Task started", 0x95a5a6),
        (Some("pr_opened"), _) => (config.notify_pr_opened, "PR opened", 0x3498db),
        (Some("merged"), _) => (config.notify_merged, "Merged", 0x2ecc71),
        (Some("task_failed"), _) => (config.notify_task_finished, "Task failed", 0xc0392b),
        (Some("task_escalated"), _) => (config.notify_task_finished, "Task escalated", 0xd35400),
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

pub fn digest(config: &DiscordConfig, entries: &[DecisionEntry]) -> Option<Notification> {
    let enabled = entries
        .iter()
        .filter(|entry| entry.source.as_deref() != Some("discord"))
        .filter(|entry| match entry.kind {
            DecisionKind::CircuitOpen => config.notify_circuit,
            DecisionKind::Escalate => config.notify_escalation,
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    let description = crate::overseer::logging::coalesce_digest(&enabled)?;
    Some(Notification {
        title: "Overseer digest".into(),
        description,
        color: 0xf1c40f,
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

    #[test]
    fn task_started_is_gated_by_its_own_toggle() {
        let mut entry = DecisionEntry::new(DecisionKind::Dispatch, "task_started");
        entry.source = Some("daemon_event".into());

        let mut config = DiscordConfig::default();
        assert!(from_decision(&config, &entry).is_some());

        config.notify_task_started = false;
        assert!(from_decision(&config, &entry).is_none());
    }

    #[test]
    fn task_failed_and_task_escalated_share_the_finished_toggle() {
        let mut failed = DecisionEntry::new(DecisionKind::Hold, "task_failed");
        failed.source = Some("daemon_event".into());
        let mut escalated = DecisionEntry::new(DecisionKind::Escalate, "task_escalated");
        escalated.source = Some("daemon_event".into());

        let mut config = DiscordConfig::default();
        assert!(from_decision(&config, &failed).is_some());
        assert!(from_decision(&config, &escalated).is_some());

        config.notify_task_finished = false;
        assert!(from_decision(&config, &failed).is_none());
        assert!(from_decision(&config, &escalated).is_none());
    }

    #[test]
    fn task_escalated_does_not_fall_through_to_the_generic_escalation_toggle() {
        let mut entry = DecisionEntry::new(DecisionKind::Escalate, "task_escalated");
        entry.source = Some("daemon_event".into());

        let mut config = DiscordConfig::default();
        config.notify_escalation = false;
        assert!(
            from_decision(&config, &entry).is_some(),
            "task_escalated must be gated by notify_task_finished, not notify_escalation"
        );
    }

    #[test]
    fn escalation_digest_is_one_notification() {
        let entries = (0..3)
            .map(|index| DecisionEntry::new(DecisionKind::Escalate, format!("blocked-{index}")))
            .collect::<Vec<_>>();
        let notification = digest(&DiscordConfig::default(), &entries).unwrap();
        assert!(notification.description.starts_with("3 overseer alert(s):"));
        assert_eq!(notification.description.lines().count(), 1);
    }
}
