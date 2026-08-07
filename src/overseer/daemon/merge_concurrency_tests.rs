use super::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

#[test]
fn empty_items_return_no_results() {
    let results = run_bounded::<u32, u32, _>(Vec::new(), 4, |item| item);
    assert!(results.is_empty());
}

#[test]
fn results_come_back_in_input_order_regardless_of_completion_order() {
    // Earlier items sleep longer, so completion order is reversed from input
    // order unless `run_bounded` restores it explicitly.
    let items: Vec<u64> = vec![30, 20, 10, 0];
    let results = run_bounded(items.clone(), items.len(), |millis| {
        std::thread::sleep(Duration::from_millis(millis));
        millis
    });
    assert_eq!(results, items);
}

#[test]
fn concurrency_never_exceeds_the_ceiling() {
    let active = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let items: Vec<u32> = (0..12).collect();
    run_bounded(items, 3, |item| {
        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
        peak.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(15));
        active.fetch_sub(1, Ordering::SeqCst);
        item
    });
    assert!(
        peak.load(Ordering::SeqCst) <= 3,
        "peak concurrency exceeded the ceiling"
    );
}

/// The property the auto-merge pass needs: one repository's slow `gh` call
/// must not delay another repository's already-running evaluation. With
/// enough threads for every item to start at once, the fast items must finish
/// in roughly their own duration, not the slow item's.
#[test]
fn a_slow_item_does_not_delay_the_others() {
    let start = Instant::now();
    let items = vec!["slow", "a", "b", "c"];
    let results = run_bounded(items.clone(), items.len(), |item| {
        if item == "slow" {
            std::thread::sleep(Duration::from_millis(250));
        }
        (item, start.elapsed())
    });
    for (item, elapsed) in &results {
        if *item != "slow" {
            assert!(
                elapsed.as_millis() < 150,
                "{item} took {elapsed:?} — should have run concurrently with the slow item, not behind it"
            );
        }
    }
}

#[test]
fn a_ceiling_above_the_item_count_still_runs_every_item_once() {
    let items: Vec<u32> = (0..3).collect();
    let mut results = run_bounded(items, 100, |item| item * 2);
    results.sort_unstable();
    assert_eq!(results, vec![0, 2, 4]);
}

#[test]
fn a_zero_ceiling_still_makes_progress() {
    let items: Vec<u32> = (0..5).collect();
    let mut results = run_bounded(items, 0, |item| item);
    results.sort_unstable();
    assert_eq!(results, vec![0, 1, 2, 3, 4]);
}
