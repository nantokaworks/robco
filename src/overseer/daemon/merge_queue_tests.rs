use super::*;

#[test]
fn first_claim_wins_the_slot() {
    let mut heads = Heads::new();
    assert!(heads.claim("/repo"));
}

#[test]
fn a_second_claim_for_the_same_repo_this_pass_is_refused() {
    let mut heads = Heads::new();
    assert!(heads.claim("/repo"));
    assert!(!heads.claim("/repo"));
    assert!(!heads.claim("/repo"));
}

#[test]
fn a_single_pull_request_is_unaffected_by_the_queue() {
    // The single-pull-request path must behave exactly as it did before this
    // module existed: nothing has claimed the repository yet, so its one
    // pull request always gets the slot.
    let mut heads = Heads::new();
    assert!(heads.claim("/only-repo"));
}

#[test]
fn repositories_do_not_share_a_slot() {
    let mut heads = Heads::new();
    assert!(heads.claim("/repo-a"));
    assert!(heads.claim("/repo-b"));
    assert!(!heads.claim("/repo-a"));
    assert!(!heads.claim("/repo-b"));
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
    assert!(heads.claim("/repo"));
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
        for _ in &open {
            if heads.claim("/repo") {
                total_updates += 1;
            }
        }
        open.remove(0);
    }
    assert_eq!(total_updates, 3);
}
