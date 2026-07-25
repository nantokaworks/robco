//! The barrier's whole contract is *when* it comes down, so every test here
//! plays out a sequence of passes rather than a single call.

use super::*;

const MAX_PASSES: u32 = 3;

fn pulled(repos: &[&str]) -> HashSet<String> {
    repos.iter().map(|repo| (*repo).to_string()).collect()
}

/// One auto-merge pass's worth of barrier bookkeeping, in the order
/// `auto_merge_pass` runs it.
fn pass(settling: &mut Settling, repos_pulled: &[&str]) -> Vec<String> {
    let settled = settle(settling, &pulled(repos_pulled));
    age(settling);
    settled
}

/// The case the barrier exists for: a second pull request in the repository
/// that just merged waits for the pull, across passes, and only then merges.
#[test]
fn a_second_merge_waits_for_the_first_pull_across_passes() {
    let mut settling = Settling::new();
    begin(&mut settling, "/repo");
    assert_eq!(barrier(&mut settling, "/repo", MAX_PASSES), Barrier::Held);

    // A whole pass goes by with no pull yet: still held.
    assert!(pass(&mut settling, &[]).is_empty());
    assert_eq!(barrier(&mut settling, "/repo", MAX_PASSES), Barrier::Held);

    // The pull lands, and with it the barrier.
    assert_eq!(pass(&mut settling, &["/repo"]), vec!["/repo".to_string()]);
    assert_eq!(barrier(&mut settling, "/repo", MAX_PASSES), Barrier::Open);
}

/// The defect this replaces: the old per-pass set expired on its own, so a pull
/// that never succeeded still let the next merge through.
#[test]
fn a_pull_that_keeps_failing_keeps_the_barrier_up() {
    let mut settling = Settling::new();
    begin(&mut settling, "/repo");
    for _ in 0..MAX_PASSES - 1 {
        assert!(pass(&mut settling, &[]).is_empty());
        assert_eq!(barrier(&mut settling, "/repo", MAX_PASSES), Barrier::Held);
    }
}

/// The wait is bounded: a pull that never succeeds must not park the repository
/// forever, and the release says so rather than looking like a normal merge.
#[test]
fn the_wait_is_bounded_and_the_release_is_reported_once() {
    let mut settling = Settling::new();
    begin(&mut settling, "/repo");
    for _ in 0..MAX_PASSES {
        pass(&mut settling, &[]);
    }
    assert_eq!(barrier(&mut settling, "/repo", MAX_PASSES), Barrier::Lifted);
    // Reporting it again on every later pass would bury the log, and there is
    // no barrier left to lift.
    assert_eq!(barrier(&mut settling, "/repo", MAX_PASSES), Barrier::Open);
}

/// Repositories have unrelated base branches, so one settling never holds
/// another.
#[test]
fn repositories_are_independent() {
    let mut settling = Settling::new();
    begin(&mut settling, "/repo-a");
    assert_eq!(barrier(&mut settling, "/repo-a", MAX_PASSES), Barrier::Held);
    assert_eq!(barrier(&mut settling, "/repo-b", MAX_PASSES), Barrier::Open);

    begin(&mut settling, "/repo-b");
    assert_eq!(
        pass(&mut settling, &["/repo-b"]),
        vec!["/repo-b".to_string()]
    );
    assert_eq!(barrier(&mut settling, "/repo-a", MAX_PASSES), Barrier::Held);
    assert_eq!(barrier(&mut settling, "/repo-b", MAX_PASSES), Barrier::Open);
}

/// A repository that entered settling during this pass has waited zero passes,
/// so the merge that raised the barrier is never itself charged for it.
#[test]
fn a_barrier_raised_this_pass_starts_its_wait_at_zero() {
    let mut settling = Settling::new();
    pass(&mut settling, &[]);
    begin(&mut settling, "/repo");
    assert_eq!(settling["/repo"].passes_held, 0);
}
