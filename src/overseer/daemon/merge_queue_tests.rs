use super::*;

#[test]
fn first_claim_wins_the_slot() {
    let mut heads = Heads::new();
    assert!(heads.claim("/repo", "a"));
}

#[test]
fn a_second_claim_for_the_same_repo_this_pass_is_refused() {
    let mut heads = Heads::new();
    assert!(heads.claim("/repo", "a"));
    assert!(!heads.claim("/repo", "b"));
    assert!(!heads.claim("/repo", "c"));
}

#[test]
fn a_single_pull_request_is_unaffected_by_the_queue() {
    // The single-pull-request path must behave exactly as it did before this
    // module existed: nothing has claimed the repository yet, so its one
    // pull request always gets the slot.
    let mut heads = Heads::new();
    assert!(heads.claim("/only-repo", "a"));
}

#[test]
fn repositories_do_not_share_a_slot() {
    let mut heads = Heads::new();
    assert!(heads.claim("/repo-a", "a"));
    assert!(heads.claim("/repo-b", "b"));
    assert!(!heads.claim("/repo-a", "c"));
    assert!(!heads.claim("/repo-b", "d"));
}

#[test]
fn a_pull_request_never_asked_to_claim_does_not_occupy_the_slot() {
    // A `Held` pull request — blocked, failing, or otherwise stuck — never
    // calls `claim` at all (see `merge_apply::merge_state_cleared`). Modelled
    // here by simply not calling it: the slot must still be free for the
    // next pull request behind it, so the order skips the stuck one instead
    // of stalling on it.
    let mut heads = Heads::new();
    // entry A: blocked/failing/held — never calls `claim`.
    // entry B, behind A in queue order, is the first to actually call it.
    assert!(heads.claim("/repo", "b"));
}

#[test]
fn releasing_the_slot_lets_the_pull_request_behind_it_claim_it_in_the_same_pass() {
    // The head merged partway through the pass. It is no longer in the queue, so
    // it can no longer invalidate anyone's work — and the pull request behind it
    // must be able to start catching up now rather than a poll interval later.
    let mut heads = Heads::new();
    assert!(heads.claim("/repo", "a"));
    assert!(!heads.claim("/repo", "b"));
    heads.release("/repo", "a");
    assert!(heads.claim("/repo", "b"));
    // Still one at a time: releasing promotes the next one, it does not open the
    // slot to everybody behind it.
    assert!(!heads.claim("/repo", "c"));
}

#[test]
fn only_the_entry_holding_the_slot_can_give_it_back() {
    // The load-bearing half of `release`. `auto_merge_pass` calls it for every
    // entry that reached a terminal phase this pass, and most of those never
    // claimed anything: a pull request GitHub reports closed stops before the
    // gate, and the hold cap escalates entries held on `checks_not_green` or
    // `merge_state:dirty`, neither of which claims. If any of them could free the
    // slot, the real head would be mid-branch-update while a third pull request
    // claimed and updated too — two check runs for a base only one can merge
    // onto, which is the cascade this module exists to prevent.
    let mut heads = Heads::new();
    assert!(heads.claim("/repo", "the-head"));

    // B never claimed, then escalated. Its release must be a no-op.
    heads.release("/repo", "b-escalated-without-claiming");
    assert!(!heads.free("/repo"));
    assert!(!heads.claim("/repo", "c"), "c must still wait its turn");

    // Only the recorded holder can hand it on.
    heads.release("/repo", "the-head");
    assert!(heads.claim("/repo", "c"));
}

#[test]
fn two_attempts_at_one_task_do_not_share_a_slot() {
    // A re-dispatched task pushes a *second* ledger entry carrying the same
    // `task_id` (`dispatch::worker::record_attempt`), and the first stays in the
    // ledger with its pull request still open — an escalated entry is
    // re-dispatchable while the merge gate is still reconsidering it. So the
    // holder token has to be the agent id, which names one entry; keyed on the
    // task id, the second attempt escalating would free the slot the *first*
    // attempt is holding mid-branch-update, and the next pull request would
    // update its branch too.
    let mut heads = Heads::new();
    assert!(heads.claim("/repo", "agent-attempt-1"));

    // Attempt 2 of the same task escalates without ever claiming.
    heads.release("/repo", "agent-attempt-2");

    assert!(!heads.free("/repo"), "attempt 1 still holds the slot");
    assert!(!heads.claim("/repo", "agent-c"));
}

#[test]
fn releasing_one_repository_leaves_every_other_repositorys_slot_alone() {
    let mut heads = Heads::new();
    assert!(heads.claim("/repo-a", "a"));
    assert!(heads.claim("/repo-b", "b"));
    heads.release("/repo-a", "a");
    assert!(heads.claim("/repo-a", "c"));
    assert!(!heads.claim("/repo-b", "d"));
}

#[test]
fn free_reports_the_slot_without_taking_it() {
    // The merge-settle barrier reads this before the gate reaches the claim, to
    // decide whether an entry is worth a GitHub read at all. Reading it must not
    // spend the slot the gate is about to claim.
    let mut heads = Heads::new();
    assert!(heads.free("/repo"));
    assert!(heads.free("/repo"));
    assert!(heads.claim("/repo", "a"));
    assert!(!heads.free("/repo"));
    heads.release("/repo", "a");
    assert!(heads.free("/repo"));
}

#[test]
fn a_repository_drains_in_one_pass_once_each_head_gives_its_slot_back() {
    // Same three pull requests as the test below, but now each pass merges the
    // head and releases the slot instead of ending there. Every pull request
    // still updates exactly once — the no-wasted-CI property is untouched — but
    // all three updates happen in the pass that has work for them, rather than
    // one per pass over three poll intervals.
    let mut heads = Heads::new();
    let mut updates = 0;
    for task in ["A", "B", "C"] {
        assert!(
            heads.claim("/repo", task),
            "the head of the queue always acts"
        );
        updates += 1;
        // ...merges, and so leaves the queue.
        heads.release("/repo", task);
    }
    assert_eq!(updates, 3);
}

#[test]
fn three_mergeable_pull_requests_cost_three_updates_total_not_six() {
    // Simulates the cascade from the task's motivating trace: three mergeable
    // pull requests (A, B, C) open in one repository, in queue order. Each
    // simulated pass claims the head slot for whichever pull requests are
    // still open, then "merges" the head (removing it), mirroring one real
    // auto-merge pass followed by the settle barrier lifting once that merge
    // lands. Before this module, every open pull request updated its branch
    // every pass: 3 + 2 + 1 = 6 updates to land all three. With the queue,
    // only the head updates each pass: 1 + 1 + 1 = 3.
    let mut open = vec!["A", "B", "C"];
    let mut total_updates = 0;
    while !open.is_empty() {
        let mut heads = Heads::new();
        for task in &open {
            if heads.claim("/repo", task) {
                total_updates += 1;
            }
        }
        open.remove(0);
    }
    assert_eq!(total_updates, 3);
}
