//! Dedupes and orders the rows a fetch collected, split out from the walk
//! itself so `repo_tasks.rs` stays about *what* got asked, not how the
//! answer is sorted for display.

use std::collections::HashSet;

use super::DroprTaskFetch;

/// Dedupes and orders the rows the fetch collected.
///
/// Ordering is what decides which rows survive the pane's display cap, so it
/// puts the most urgent first: priority, then task number so the tie-break
/// reads as task order.
pub(super) fn settle(mut fetch: DroprTaskFetch) -> DroprTaskFetch {
    let mut seen = HashSet::new();
    fetch
        .tasks
        .retain(|task| seen.insert(task.display_id.clone()));
    fetch.tasks.sort_by_key(|task| {
        (
            priority_rank(&task.priority),
            display_number(&task.display_id),
        )
    });
    fetch
}

/// Display order for priorities, most urgent first. An unset or unrecognised
/// priority sorts last: it is the least informative row to keep.
fn priority_rank(priority: &str) -> u8 {
    match priority {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    }
}

/// The number in a `#N` display id, so `#9` sorts before `#10`.
pub(super) fn display_number(display_id: &str) -> u64 {
    display_id
        .trim_start_matches('#')
        .split('-')
        .next()
        .unwrap_or_default()
        .parse()
        .unwrap_or(u64::MAX)
}
