use super::*;

/// The filter → dedup → render composition `notify_plan::next_notification`
/// runs for a digest-sized burst, without the distinct-count gate.
fn digest(config: &DiscordConfig, entries: &[DecisionEntry]) -> Option<Notification> {
    digest_with_ids(config, entries, &HashMap::new())
}

fn digest_with_ids(
    config: &DiscordConfig,
    entries: &[DecisionEntry],
    display_ids: &HashMap<String, String>,
) -> Option<Notification> {
    let alerts = dedup_alerts(digest_alerts(config, entries));
    (!alerts.is_empty()).then(|| digest_notification(&alerts, display_ids))
}

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

/// `overseer::release_pipeline` wraps its own words: the prefix picks the
/// title/tier/color here, and `humanize::sentence` separately strips that
/// same prefix for the description.
#[test]
fn a_published_release_reads_as_a_summary_tier_notification() {
    let mut entry = DecisionEntry::new(
        DecisionKind::Release,
        "release_published:Published RobCo 0.1.93.",
    );
    entry.source = Some("daemon_event".into());
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert_eq!(notification.title, "Release published");
    assert_eq!(notification.description, "Published RobCo 0.1.93.");
    assert_eq!(notification.color, 0x2ecc71);
}

#[test]
fn a_failed_release_reads_as_an_errors_tier_notification() {
    let mut entry = DecisionEntry::new(
        DecisionKind::Release,
        "release_failed:Failed at stage=check cargo test failed.",
    );
    entry.source = Some("daemon_event".into());
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert_eq!(notification.title, "Release failed");
    assert_eq!(
        notification.description,
        "Failed at stage=check cargo test failed."
    );
    assert_eq!(notification.color, 0xc0392b);
}

/// The regression this covers: a `release_pipeline_skipped` decision used to
/// carry the plain `daemon` source, which `from_decision` never treats as a
/// named event, so it fell through to `_ => return None` and never reached
/// Discord — a checkout stuck off `ready()` could block every release after
/// it for days with nothing but a decision-log line to notice it by. It now
/// uses the same `daemon_event` source `release_published` / `release_failed`
/// do, so it reaches the same Errors-tier notification path they do.
#[test]
fn a_skipped_release_reads_as_an_errors_tier_notification() {
    let mut entry = DecisionEntry::new(
        DecisionKind::Skip,
        "release_pipeline_skipped:checkout_not_on_merged_commit",
    );
    entry.source = Some("daemon_event".into());
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert_eq!(notification.title, "Release skipped");
    assert_eq!(
        notification.description,
        "The release checkout is not on the merged commit, so the release did not run."
    );
    assert_eq!(notification.color, 0xe67e22);
}

/// dropr:444 — a checkout left off `main` names the branch it is on and the
/// fix (return it to `main`), not just the bare reason code.
#[test]
fn a_skipped_release_off_main_names_the_branch_and_the_fix() {
    let mut entry = DecisionEntry::new(
        DecisionKind::Skip,
        "release_pipeline_skipped:checkout_not_on_main:operator-wip",
    );
    entry.source = Some("daemon_event".into());
    let notification = from_decision(&DiscordConfig::default(), &entry).unwrap();
    assert_eq!(notification.title, "Release skipped");
    assert_eq!(
        notification.description,
        "The release checkout is on another branch, not main, so the release did not run. \
         Move the checkout back to main to unblock the release."
    );
    assert_eq!(
        field_value(&notification, "Reason"),
        Some("`release_pipeline_skipped:checkout_not_on_main:operator-wip`")
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
    // Distinct reasons, so dedup keeps all fifteen lines in play.
    let entries = (0..15)
        .map(|index| {
            DecisionEntry::new(
                DecisionKind::Escalate,
                format!("{index}-{}", "y".repeat(1000)),
            )
        })
        .collect::<Vec<_>>();
    let notification = digest(&DiscordConfig::default(), &entries).unwrap();
    assert!(notification.description.chars().count() <= 4096);
    assert!(notification.description.contains("more"));
}

/// The acceptance shape: repo short name, dropr display id, clickable PR
/// link, humanized reason, and a `×N` count instead of repeated lines.
#[test]
fn a_digest_line_carries_repo_display_id_pr_link_and_a_humanized_reason() {
    let mut entry = DecisionEntry::new(DecisionKind::Escalate, "autonomy_envelope");
    entry.task = Some("vm04Y9jTaBcDeFgHiJkLm".into());
    entry.repo = Some("/home/op/.robco/repos/nex".into());
    entry.pr_url = Some("https://github.com/acme/nex/pull/766".into());
    let entries = vec![entry.clone(), entry.clone(), entry];
    let display_ids = HashMap::from([("vm04Y9jTaBcDeFgHiJkLm".to_string(), "#490".to_string())]);
    let notification = digest_with_ids(&DiscordConfig::default(), &entries, &display_ids).unwrap();
    let lines: Vec<_> = notification.description.lines().collect();
    assert_eq!(lines[0], "**1 overseer alert(s)**");
    assert_eq!(
        lines[1],
        "- nex · #490 [#766](https://github.com/acme/nex/pull/766): \
         The autonomy policy held this merge for an operator. \
         Approve it from the TUI inbox or with `robco_approve` on Discord. ×3"
    );
    assert_eq!(lines.len(), 2);
}

/// A task the ledger no longer knows falls back to the backticked nanoid;
/// the reason still humanizes.
#[test]
fn a_digest_line_falls_back_to_the_nanoid_when_no_display_id_resolves() {
    let mut entry = DecisionEntry::new(DecisionKind::Escalate, "autonomy_envelope");
    entry.task = Some("vm04Y9jTaBcDeFgHiJkLm".into());
    let notification = digest(&DiscordConfig::default(), &[entry]).unwrap();
    assert!(
        notification
            .description
            .contains("- `vm04Y9jTaBcDeFgHiJkLm`: The autonomy policy held this merge")
    );
}

/// Dedup keys on (task, reason): the same reason on two tasks stays two
/// lines, and the header counts distinct alerts, not raw entries.
#[test]
fn dedup_keeps_the_same_reason_apart_across_tasks() {
    let mut first = DecisionEntry::new(DecisionKind::Escalate, "autonomy_envelope");
    first.task = Some("task-a".into());
    let mut second = DecisionEntry::new(DecisionKind::Escalate, "autonomy_envelope");
    second.task = Some("task-b".into());
    let notification = digest(&DiscordConfig::default(), &[first.clone(), second, first]).unwrap();
    let lines: Vec<_> = notification.description.lines().collect();
    assert_eq!(lines[0], "**2 overseer alert(s)**");
    assert!(lines[1].starts_with("- `task-a`:"));
    assert!(lines[1].ends_with("×2"));
    assert!(lines[2].starts_with("- `task-b`:"));
    assert!(!lines[2].ends_with("×2"));
}

/// The dropr:397 retuning: `summary` means milestones only — a merge and
/// every Errors-tier event. Step-by-step narration (`task_started`,
/// `queue_drained`, `pr_opened`) is all-tier and stays silent at the default.
#[test]
fn summary_level_admits_merges_and_errors_but_not_step_events() {
    let config = DiscordConfig::default();
    assert_eq!(
        config.notify_level,
        crate::overseer::config::NotifyLevel::Summary
    );

    for (reason, kind) in [
        ("merged", DecisionKind::Merge),
        ("task_failed", DecisionKind::Hold),
    ] {
        let mut entry = DecisionEntry::new(kind, reason);
        entry.source = Some("daemon_event".into());
        assert!(
            from_decision(&config, &entry).is_some(),
            "{reason} should fire at the summary default"
        );
    }

    for (reason, kind) in [
        ("task_started", DecisionKind::Dispatch),
        ("queue_drained", DecisionKind::Skip),
        ("pr_opened", DecisionKind::Hold),
    ] {
        let mut entry = DecisionEntry::new(kind, reason);
        entry.source = Some("daemon_event".into());
        assert!(
            from_decision(&config, &entry).is_none(),
            "{reason} is all-tier and must stay silent at the summary default"
        );
    }
}

#[test]
fn all_level_admits_the_step_events_summary_silences() {
    let config = DiscordConfig {
        notify_level: crate::overseer::config::NotifyLevel::All,
        ..DiscordConfig::default()
    };
    for (reason, kind) in [
        ("task_started", DecisionKind::Dispatch),
        ("queue_drained", DecisionKind::Skip),
        ("pr_opened", DecisionKind::Hold),
    ] {
        let mut entry = DecisionEntry::new(kind, reason);
        entry.source = Some("daemon_event".into());
        assert!(
            from_decision(&config, &entry).is_some(),
            "{reason} should fire at all"
        );
    }
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
            "{reason} is above the errors level and must stay silent"
        );
    }

    let mut failed = DecisionEntry::new(DecisionKind::Hold, "task_failed");
    failed.source = Some("daemon_event".into());
    assert!(from_decision(&config, &failed).is_some());
}
