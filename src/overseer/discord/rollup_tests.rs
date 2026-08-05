use super::*;
use crate::overseer::config::NotifyLevel;
use crate::overseer::logging::DecisionKind;

fn merged_entry(repo: &str, url: &str) -> DecisionEntry {
    let mut entry = DecisionEntry::new(DecisionKind::Merge, "merged");
    entry.source = Some("daemon_event".into());
    entry.repo = Some(repo.into());
    entry.pr_url = Some(url.into());
    entry
}

fn queue(entries: Vec<DecisionEntry>) -> VecDeque<PendingDecision> {
    entries
        .into_iter()
        .map(|entry| PendingDecision::stub(Some(entry)))
        .collect()
}

fn consumed(planned: Option<Planned>) -> (usize, Notification) {
    match planned.expect("a merge at the front must be planned here") {
        Planned::Consume {
            count,
            notification,
        } => (count, notification.expect("an admitted merge notifies")),
        Planned::Hold => panic!("expected a consuming plan, got a hold"),
    }
}

#[test]
fn a_lone_fresh_merge_is_held_for_the_window() {
    let now = Utc::now();
    let pending = queue(vec![merged_entry("acme/one", "https://x.test/pull/1")]);
    assert!(matches!(
        plan_merged(&DiscordConfig::default(), &pending, now),
        Some(Planned::Hold)
    ));
}

#[test]
fn a_lone_merge_past_the_window_sends_the_normal_single_message() {
    let now = Utc::now();
    let mut entry = merged_entry("acme/one", "https://x.test/pull/1");
    entry.at = now - Duration::minutes(WINDOW_MINUTES + 1);
    let pending = queue(vec![entry]);
    let (count, notification) = consumed(plan_merged(&DiscordConfig::default(), &pending, now));
    assert_eq!(count, 1);
    assert_eq!(notification.description, "The pull request was merged.");
}

#[test]
fn merges_within_the_window_roll_up_into_one_message() {
    let now = Utc::now();
    let mut first = merged_entry("acme/one", "https://x.test/pull/312");
    first.at = now - Duration::minutes(WINDOW_MINUTES + 1);
    let second = merged_entry("acme/two", "https://x.test/pull/767");
    let third = merged_entry("acme/three", "https://x.test/pull/9");
    let pending = queue(vec![first, second, third]);
    let (count, notification) = consumed(plan_merged(&DiscordConfig::default(), &pending, now));
    assert_eq!(count, 3);
    assert_eq!(notification.description, "3 pull requests were merged.");
    assert_eq!(notification.fields.len(), 1);
    assert_eq!(notification.fields[0].name, "PRs");
    assert_eq!(
        notification.fields[0].value,
        "[acme/one #312](https://x.test/pull/312), \
         [acme/two #767](https://x.test/pull/767), \
         [acme/three #9](https://x.test/pull/9)"
    );
}

/// An error queued behind held merges flushes them at once: the rollup may
/// delay merges, never the errors behind them.
#[test]
fn a_notifying_event_behind_held_merges_flushes_them_immediately() {
    let now = Utc::now();
    let mut failed = DecisionEntry::new(DecisionKind::Hold, "task_failed");
    failed.source = Some("daemon_event".into());
    let pending = queue(vec![
        merged_entry("acme/one", "https://x.test/pull/1"),
        merged_entry("acme/two", "https://x.test/pull/2"),
        failed,
    ]);
    let (count, notification) = consumed(plan_merged(&DiscordConfig::default(), &pending, now));
    // Only the merges are consumed; the error plans on its own next.
    assert_eq!(count, 2);
    assert_eq!(notification.description, "2 pull requests were merged.");
}

/// Entries the level silences (here `task_started` at the `summary`
/// default) ride along inside the rollup group instead of splitting it.
#[test]
fn silent_entries_ride_along_with_the_rollup() {
    let now = Utc::now();
    let mut first = merged_entry("acme/one", "https://x.test/pull/1");
    first.at = now - Duration::minutes(WINDOW_MINUTES + 1);
    let mut started = DecisionEntry::new(DecisionKind::Dispatch, "task_started");
    started.source = Some("daemon_event".into());
    let pending = queue(vec![
        first,
        started,
        merged_entry("acme/two", "https://x.test/pull/2"),
    ]);
    let (count, notification) = consumed(plan_merged(&DiscordConfig::default(), &pending, now));
    assert_eq!(count, 3);
    assert_eq!(notification.description, "2 pull requests were merged.");
}

#[test]
fn a_non_merge_front_hands_planning_back() {
    let mut failed = DecisionEntry::new(DecisionKind::Hold, "task_failed");
    failed.source = Some("daemon_event".into());
    let pending = queue(vec![failed]);
    assert!(plan_merged(&DiscordConfig::default(), &pending, Utc::now()).is_none());
}

/// A level that silences merges (`errors`) never enters the rollup: the
/// caller's normal path consumes the entry silently instead of holding it.
#[test]
fn a_level_that_silences_merges_hands_planning_back() {
    let config = DiscordConfig {
        notify_level: NotifyLevel::Errors,
        ..DiscordConfig::default()
    };
    let pending = queue(vec![merged_entry("acme/one", "https://x.test/pull/1")]);
    assert!(plan_merged(&config, &pending, Utc::now()).is_none());
}

#[test]
fn merge_labels_degrade_to_whatever_the_entry_carries() {
    let mut no_url = DecisionEntry::new(DecisionKind::Merge, "merged");
    no_url.repo = Some("acme/one".into());
    assert_eq!(merge_label(&no_url), "acme/one");

    let mut task_only = DecisionEntry::new(DecisionKind::Merge, "merged");
    task_only.task = Some("task-42".into());
    assert_eq!(merge_label(&task_only), "task-42");

    let mut odd_url = DecisionEntry::new(DecisionKind::Merge, "merged");
    odd_url.pr_url = Some("https://x.test/x".into());
    assert_eq!(
        merge_label(&odd_url),
        "[https://x.test/x](https://x.test/x)"
    );
}
