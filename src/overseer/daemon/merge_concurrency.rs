//! Bounded concurrent execution of one repository's turn through the
//! auto-merge pass.
//!
//! Every blocking `gh`/`git` call the pass makes (`run_timeout`, throughout
//! `daemon/`) is a plain `std::process::Command`, not an async future, so
//! there is no async runtime to hand this work to. This pool mirrors that
//! shape — a fixed number of `std::thread` handles, refilled from a queue —
//! rather than introducing a second concurrency idiom.

use std::sync::Mutex;

/// Runs `work` once for each item in `items`, using up to `ceiling` OS threads
/// at a time, and returns one result per item in the same order as `items`
/// (not completion order).
///
/// Threads pull from a shared queue rather than each owning a fixed slice, so
/// a thread that finishes its item quickly picks up the next one immediately.
/// One slow item therefore delays only the threads still queued behind it —
/// never an item a different, still-free thread already claimed. This is the
/// property the auto-merge pass needs: one repository's slow `gh` call must
/// not hold back another repository's already-running evaluation.
///
/// A panic inside `work` unwinds the worker thread that ran it; `thread::scope`
/// re-raises it in the caller once every thread has been joined, exactly as an
/// unthreaded call to `work` would have propagated it.
pub(super) fn run_bounded<T, R, F>(items: Vec<T>, ceiling: usize, work: F) -> Vec<R>
where
    T: Send,
    R: Send,
    F: Fn(T) -> R + Sync,
{
    if items.is_empty() {
        return Vec::new();
    }
    let ceiling = ceiling.max(1).min(items.len());
    let queue = Mutex::new(items.into_iter().enumerate());
    let results = Mutex::new(Vec::with_capacity(ceiling));
    std::thread::scope(|scope| {
        for _ in 0..ceiling {
            scope.spawn(|| {
                while let Some((index, item)) = {
                    let mut queue = queue.lock().unwrap();
                    queue.next()
                } {
                    let result = work(item);
                    results.lock().unwrap().push((index, result));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

#[cfg(test)]
#[path = "merge_concurrency_tests.rs"]
mod tests;
