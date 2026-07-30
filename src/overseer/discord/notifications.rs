use crate::overseer::{
    config::{DiscordConfig, NotifyTier},
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
        // `pr_opened` is `all`-tier, above the `summary` default, so this
        // exercises it at the level that admits it.
        let config = DiscordConfig {
            notify_level: crate::overseer::config::NotifyLevel::All,
            ..DiscordConfig::default()
        };
        let notification = from_decision(&config, &entry).unwrap();
        assert!(notification.description.contains("task-135"));
        assert!(
            notification
                .description
                .contains("https://example.test/pull/1")
        );
    }

    #[test]
    fn task_failed_and_task_escalated_are_errors_tier() {
        let mut failed = DecisionEntry::new(DecisionKind::Hold, "task_failed");
        failed.source = Some("daemon_event".into());
        let mut escalated = DecisionEntry::new(DecisionKind::Escalate, "task_escalated");
        escalated.source = Some("daemon_event".into());

        let config = DiscordConfig::default();
        assert!(from_decision(&config, &failed).is_some());
        assert!(from_decision(&config, &escalated).is_some());

        let off = DiscordConfig {
            notify_level: crate::overseer::config::NotifyLevel::Off,
            ..DiscordConfig::default()
        };
        assert!(from_decision(&off, &failed).is_none());
        assert!(from_decision(&off, &escalated).is_none());
    }

    /// The suppression `merge_escalation` decides at decision-write time: a
    /// merge escalation the recheck loop may still resolve on its own must
    /// not reach Discord, regardless of which tier it would otherwise match.
    #[test]
    fn a_transient_merge_escalation_is_suppressed() {
        let mut entry = DecisionEntry::new(DecisionKind::Escalate, "merge_hold_cap_reached:x");
        entry.source = Some("auto_merge".into());
        entry.escalation_notify = Some(false);
        assert!(from_decision(&DiscordConfig::default(), &entry).is_none());
    }

    #[test]
    fn a_terminal_merge_escalation_still_notifies() {
        let mut entry = DecisionEntry::new(DecisionKind::Escalate, "merge_recovery_cap_reached");
        entry.source = Some("merge_recovery".into());
        entry.escalation_notify = Some(true);
        assert!(from_decision(&DiscordConfig::default(), &entry).is_some());
    }

    /// A decision outside the merge-escalation vocabulary carries no tag at
    /// all, so it must notify exactly as it did before the tag existed.
    #[test]
    fn an_untagged_escalate_decision_is_unaffected() {
        let mut entry = DecisionEntry::new(DecisionKind::Escalate, "worker blocked");
        entry.source = Some("reconcile".into());
        assert_eq!(entry.escalation_notify, None);
        assert!(from_decision(&DiscordConfig::default(), &entry).is_some());
    }

    #[test]
    fn digest_drops_a_suppressed_transient_escalation_but_keeps_the_rest() {
        let mut suppressed = DecisionEntry::new(DecisionKind::Escalate, "merge_hold_cap_reached:x");
        suppressed.escalation_notify = Some(false);
        let kept = DecisionEntry::new(DecisionKind::Escalate, "other-alert");
        let notification = digest(&DiscordConfig::default(), &[suppressed.clone(), kept]).unwrap();
        assert!(notification.description.contains("other-alert"));
        assert!(!notification.description.contains("merge_hold_cap_reached"));

        // Suppress every entry: nothing is left to digest.
        assert!(digest(&DiscordConfig::default(), &[suppressed.clone(), suppressed]).is_none());
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

    #[test]
    fn summary_level_admits_task_started_finish_and_drain_but_not_pr_opened() {
        let config = DiscordConfig::default();
        assert_eq!(
            config.notify_level,
            crate::overseer::config::NotifyLevel::Summary
        );

        for (reason, kind) in [
            ("task_started", DecisionKind::Dispatch),
            ("merged", DecisionKind::Merge),
            ("queue_drained", DecisionKind::Skip),
            ("task_failed", DecisionKind::Hold),
        ] {
            let mut entry = DecisionEntry::new(kind, reason);
            entry.source = Some("daemon_event".into());
            assert!(
                from_decision(&config, &entry).is_some(),
                "{reason} should fire at the summary default"
            );
        }

        let mut pr_opened = DecisionEntry::new(DecisionKind::Hold, "pr_opened");
        pr_opened.source = Some("daemon_event".into());
        assert!(
            from_decision(&config, &pr_opened).is_none(),
            "pr_opened is all-tier and must stay silent at the summary default"
        );
    }

    #[test]
    fn errors_level_silences_task_started_merged_and_drain() {
        let config = DiscordConfig {
            notify_level: crate::overseer::config::NotifyLevel::Errors,
            ..DiscordConfig::default()
        };

        for (reason, kind) in [
            ("task_started", DecisionKind::Dispatch),
            ("merged", DecisionKind::Merge),
            ("queue_drained", DecisionKind::Skip),
        ] {
            let mut entry = DecisionEntry::new(kind, reason);
            entry.source = Some("daemon_event".into());
            assert!(
                from_decision(&config, &entry).is_none(),
                "{reason} is summary-tier and must stay silent at errors"
            );
        }

        let mut failed = DecisionEntry::new(DecisionKind::Hold, "task_failed");
        failed.source = Some("daemon_event".into());
        assert!(from_decision(&config, &failed).is_some());
    }
}
