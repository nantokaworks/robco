use super::*;
use crate::overseer::discord::cursor::PendingDecision;

fn pending(entries: Vec<DecisionEntry>) -> VecDeque<PendingDecision> {
    entries.into_iter().map(PendingDecision::planned).collect()
}

fn escalation(task: &str, reason: &str) -> DecisionEntry {
    let mut entry = DecisionEntry::new(DecisionKind::Escalate, reason);
    entry.task = Some(task.into());
    entry.repo = Some("acme/widgets".into());
    entry
}

/// A burst of two distinct alerts — even repeated — is not a digest: each
/// distinct alert goes out as a full notification with its own fields, and
/// the whole run is still consumed at once.
#[test]
fn a_small_escalation_burst_renders_individual_notifications() {
    let entries = vec![
        escalation("task-a", "autonomy_envelope"),
        escalation("task-b", "missing_pr_url"),
        escalation("task-a", "autonomy_envelope"),
    ];
    let (count, notifications) = next_notification(
        &DiscordConfig::default(),
        &pending(entries),
        &HashMap::new(),
    );
    assert_eq!(count, 3);
    assert_eq!(notifications.len(), 2);
    for notification in &notifications {
        assert_eq!(notification.title, "Escalation");
        assert!(
            notification.fields.iter().any(|field| field.name == "Task"),
            "an individual notification keeps its Task field"
        );
    }
}

#[test]
fn three_distinct_alerts_coalesce_into_one_digest() {
    let entries = vec![
        escalation("task-a", "autonomy_envelope"),
        escalation("task-b", "missing_pr_url"),
        escalation("task-c", "pr_closed_unmerged"),
    ];
    let (count, notifications) = next_notification(
        &DiscordConfig::default(),
        &pending(entries),
        &HashMap::new(),
    );
    assert_eq!(count, 3);
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].title, "Overseer digest");
}

/// A run whose every entry is suppressed consumes the run and plans nothing,
/// so the cursor still advances past it.
#[test]
fn a_fully_suppressed_run_plans_no_notifications() {
    let mut suppressed = escalation("task-a", "merge_hold_cap_reached:x");
    suppressed.escalation_notify = Some(false);
    let (count, notifications) = next_notification(
        &DiscordConfig::default(),
        &pending(vec![suppressed.clone(), suppressed]),
        &HashMap::new(),
    );
    assert_eq!(count, 2);
    assert!(notifications.is_empty());
}
