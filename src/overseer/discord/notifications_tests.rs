use super::*;

fn field_value<'a>(notification: &'a Notification, name: &str) -> Option<&'a str> {
    notification
        .fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
}

#[test]
fn notification_puts_task_repo_and_pr_into_fields() {
    let mut entry = DecisionEntry::new(DecisionKind::Hold, "pr_opened");
    entry.source = Some("daemon_event".into());
    entry.task = Some("task-135".into());
    entry.repo = Some("acme/widgets".into());
    entry.pr_url = Some("https://example.test/pull/1".into());
    // `pr_opened` is `all`-tier, above the `summary` default, so this
    // exercises it at the level that admits it.
    let config = DiscordConfig {
        notify_level: crate::overseer::config::NotifyLevel::All,
        ..DiscordConfig::default()
    };
    let notification = from_decision(&config, &entry).unwrap();
    assert_eq!(
        notification.description,
        "The worker opened a pull request."
    );
    assert_eq!(field_value(&notification, "Task"), Some("`task-135`"));
    assert_eq!(field_value(&notification, "Repo"), Some("`acme/widgets`"));
    assert_eq!(
        field_value(&notification, "PR"),
        Some("[#1](https://example.test/pull/1)")
    );
    assert_eq!(field_value(&notification, "Reason"), Some("`pr_opened`"));
}

/// The humanized path: a known code becomes the sentence the localizer will
/// translate, while the raw code moves into a `Reason` field the localizer
/// never touches.
#[test]
fn a_known_reason_code_reads_as_a_sentence_with_the_raw_code_secondary() {
    let mut entry = DecisionEntry::new(
        DecisionKind::Escalate,
        "merge_hold_cap_reached:checks_waiting",
    );
    entry.source = Some("auto_merge".into());
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert_eq!(
        notification.description,
        "The pull request was held on the same condition too many times."
    );
    assert_eq!(
        field_value(&notification, "Reason"),
        Some("`merge_hold_cap_reached:checks_waiting`")
    );
}

#[test]
fn an_unknown_reason_code_still_reads_verbatim_without_a_reason_field() {
    let mut entry = DecisionEntry::new(DecisionKind::Escalate, "some_future_code:detail");
    entry.source = Some("reconcile".into());
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert_eq!(notification.description, "some_future_code:detail");
    assert_eq!(field_value(&notification, "Reason"), None);
}

/// A judge's own words are already a sentence: the wrapper is stripped for
/// the description and the full wrapped reason stays in the `Reason` field.
#[test]
fn a_judge_wrapped_reason_shows_the_judge_own_words() {
    let mut entry = DecisionEntry::new(
        DecisionKind::Escalate,
        "judge_escalate:The change needs a rebase first.",
    );
    entry.source = Some("auto_merge".into());
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert_eq!(notification.description, "The change needs a rebase first.");
    assert_eq!(
        field_value(&notification, "Reason"),
        Some("`judge_escalate:The change needs a rebase first.`")
    );
}

#[test]
fn a_pr_url_without_pull_segment_still_links() {
    let mut entry = DecisionEntry::new(DecisionKind::Escalate, "stuck");
    entry.pr_url = Some("https://example.test/x".into());
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert_eq!(
        field_value(&notification, "PR"),
        Some("[https://example.test/x](https://example.test/x)")
    );
}

#[test]
fn a_notification_without_metadata_has_no_fields() {
    // An unknown, prose-shaped reason: no Task/Repo/PR metadata and no
    // humanized sentence, so nothing produces a field at all.
    let entry = DecisionEntry::new(DecisionKind::Escalate, "stuck");
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert!(notification.fields.is_empty());
}

#[test]
fn a_known_reason_without_metadata_carries_only_the_reason_field() {
    let mut entry = DecisionEntry::new(DecisionKind::Hold, "merged");
    entry.source = Some("daemon_event".into());
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert_eq!(notification.description, "The pull request was merged.");
    assert_eq!(notification.fields.len(), 1);
    assert_eq!(field_value(&notification, "Reason"), Some("`merged`"));
}

#[test]
fn an_oversized_reason_is_clipped_to_the_embed_limit() {
    let mut entry = DecisionEntry::new(DecisionKind::Escalate, "x".repeat(5000));
    entry.source = Some("reconcile".into());
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert_eq!(notification.description.chars().count(), 4096);
    assert!(notification.description.ends_with('…'));
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
fn escalation_digest_lists_one_alert_per_line() {
    let entries = (0..3)
        .map(|index| {
            let mut entry = DecisionEntry::new(DecisionKind::Escalate, format!("blocked-{index}"));
            entry.task = Some(format!("task-{index}"));
            entry
        })
        .collect::<Vec<_>>();
    let notification = digest(&DiscordConfig::default(), &entries).unwrap();
    let lines: Vec<_> = notification.description.lines().collect();
    assert_eq!(lines[0], "**3 overseer alert(s)**");
    assert_eq!(lines[1], "- `task-0`: blocked-0");
    assert_eq!(lines.len(), 4);
}

#[test]
fn an_overflowing_digest_ends_with_a_more_tail() {
    let entries = (0..15)
        .map(|index| DecisionEntry::new(DecisionKind::Escalate, format!("blocked-{index}")))
        .collect::<Vec<_>>();
    let notification = digest(&DiscordConfig::default(), &entries).unwrap();
    let lines: Vec<_> = notification.description.lines().collect();
    assert_eq!(lines[0], "**15 overseer alert(s)**");
    assert_eq!(*lines.last().unwrap(), "… and 5 more");
    // Header, ten alert lines, and the tail.
    assert_eq!(lines.len(), 12);
}

#[test]
fn a_digest_of_long_reasons_stays_under_the_embed_limit() {
    let entries = (0..15)
        .map(|_| DecisionEntry::new(DecisionKind::Escalate, "y".repeat(1000)))
        .collect::<Vec<_>>();
    let notification = digest(&DiscordConfig::default(), &entries).unwrap();
    assert!(notification.description.chars().count() <= 4096);
    assert!(notification.description.contains("more"));
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
