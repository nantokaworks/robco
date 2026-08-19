use super::*;

#[test]
fn every_tag_is_short_and_unique() {
    let moves = [
        Move::Answer,
        Move::Merge,
        Move::Retry,
        Move::Review,
        Move::Watch,
    ];
    let tags = moves.map(Move::tag);
    for tag in tags {
        assert!(tag.len() <= 6, "{tag} exceeds the 6-column budget");
    }
    let mut sorted = tags.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), tags.len(), "tags must be unique: {tags:?}");
}

#[test]
fn only_watch_is_not_actionable() {
    for step in [Move::Answer, Move::Merge, Move::Retry, Move::Review] {
        assert!(step.actionable(), "{step:?} should be actionable");
    }
    assert!(!Move::Watch.actionable());
}

#[test]
fn ledger_parked_is_a_review_remedy() {
    assert_eq!(LEDGER_PARKED.step, Move::Review);
    assert!(LEDGER_PARKED.actionable());
    assert!(!LEDGER_PARKED.means.is_empty());
    assert!(!LEDGER_PARKED.next.is_empty());
}

#[test]
fn ledger_parked_resumable_is_a_non_actionable_watch_remedy() {
    assert_eq!(LEDGER_PARKED_RESUMABLE.step, Move::Watch);
    assert!(!LEDGER_PARKED_RESUMABLE.actionable());
    assert!(!LEDGER_PARKED_RESUMABLE.means.is_empty());
    assert!(!LEDGER_PARKED_RESUMABLE.next.is_empty());
}

#[test]
fn an_unrecognised_reason_falls_back_to_operator_review() {
    // Not in any table and not classified recoverable, but still shaped like
    // a code — the prose test must not mistake it for free text.
    let remedy = resolve("something_github_added_later", true);
    assert_eq!(remedy.step, Move::Review);
    assert!(!remedy.means.is_empty());
    assert!(!remedy.next.is_empty());
}

/// dropr:375 — a pull request waiting on a prerequisite task is nothing an
/// operator needs to act on, the same way `merge_queue::WAITING_TURN` is not.
#[test]
fn a_prerequisite_wait_resolves_to_watch() {
    let remedy = resolve("prerequisite_unmerged:#7", true);
    assert_eq!(remedy.step, Move::Watch);
    assert!(!remedy.actionable());
}

/// dropr:460 — these four reasons were measured falling to the generic
/// fallback wording (or, for `merge_hold_cap_reached`, silently inheriting
/// whatever the wrapped condition said, which loses the fact that the
/// automated path already gave up on it). A later refactor that drops one of
/// these entries must not go unnoticed.
#[test]
fn each_high_volume_reason_resolves_to_its_own_entry_not_the_fallback() {
    let cases = [
        "merge_hold_cap_reached:merge_state:dirty",
        "repeating_hold: 3× checks_not_green",
        "stalled: #204 has been pr_opened for 180m in owner/repo",
        "merge_recovery_undeliverable:merge_state:dirty",
    ];
    for reason in cases {
        let remedy = resolve(reason, true);
        assert_eq!(remedy.step, Move::Review, "{reason}");
        assert_ne!(
            remedy.means, RECOVERABLE_FALLBACK.means,
            "{reason} still reads as the generic recoverable fallback"
        );
        assert_ne!(
            remedy.means, OPERATOR_FALLBACK.means,
            "{reason} still reads as the generic operator fallback"
        );
    }
}
