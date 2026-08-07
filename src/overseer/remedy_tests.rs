use super::*;

#[test]
fn every_tag_is_short_and_unique() {
    let moves = [
        Move::Answer,
        Move::Merge,
        Move::Reset,
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
    for step in [
        Move::Answer,
        Move::Merge,
        Move::Reset,
        Move::Retry,
        Move::Review,
    ] {
        assert!(step.actionable(), "{step:?} should be actionable");
    }
    assert!(!Move::Watch.actionable());
}

#[test]
fn worker_question_is_an_answerable_remedy() {
    assert_eq!(WORKER_QUESTION.step, Move::Answer);
    assert!(!WORKER_QUESTION.means.is_empty());
    assert!(!WORKER_QUESTION.next.is_empty());
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
    // a code — the prose test must not mistake it for a judge sentence.
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
