use super::{exact, merge, prefix, runtime};
use crate::overseer::remedy::{FailureClass, Move, Remedy, classify};

fn all_entries() -> Vec<(&'static str, Remedy)> {
    merge::EXACT
        .iter()
        .chain(merge::PREFIX)
        .chain(runtime::EXACT)
        .chain(runtime::PREFIX)
        .copied()
        .collect()
}

/// The anti-drift pin between the two taxonomies, scoped to `table::merge` —
/// `classify` only ever looks at merge-gate reasons (`merge_recovery` never
/// calls it on a dispatch or triage reason), so it is only meaningful to hold
/// `table::runtime`'s entries to a bar `classify` has no opinion on at all.
/// Within its own domain, every merge-gate entry must agree with `classify`
/// in both directions: a reason `classify` calls `Recoverable` must resolve
/// to `Answer`, and an `Answer` entry must be something `classify` calls
/// `Recoverable`.
#[test]
fn table_agrees_with_classify_on_every_recoverable_entry() {
    let merge_entries = merge::EXACT.iter().chain(merge::PREFIX).copied();
    for (key, remedy) in merge_entries {
        // A prefix entry's key is only ever a prefix of a real reason, so a
        // bare suffix exercises `classify`'s own `starts_with` the same way a
        // real reason would.
        let sample = if key.ends_with(':') {
            format!("{key}sample")
        } else {
            key.to_string()
        };
        let recoverable = classify(&sample) == FailureClass::Recoverable;
        let answers = remedy.step == Move::Answer;
        assert_eq!(
            recoverable, answers,
            "classify/table drift on {key:?}: classify says recoverable={recoverable}, \
             table resolves to {remedy:?}"
        );
    }
}

#[test]
fn every_table_entry_has_distinct_non_empty_guidance() {
    for (key, remedy) in all_entries() {
        assert!(!remedy.means.is_empty(), "{key:?} has an empty `means`");
        assert!(!remedy.next.is_empty(), "{key:?} has an empty `next`");
        assert_ne!(
            remedy.means, remedy.next,
            "{key:?} repeats itself between `means` and `next`"
        );
    }
}

#[test]
fn exact_keys_are_unique_across_both_domains() {
    let mut keys: Vec<&str> = merge::EXACT
        .iter()
        .chain(runtime::EXACT)
        .map(|(key, _)| *key)
        .collect();
    let total = keys.len();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(
        keys.len(),
        total,
        "an exact reason is listed more than once"
    );
}

#[test]
fn an_unmatched_reason_yields_neither_exact_nor_prefix_entry() {
    assert!(exact("something_github_added_later").is_none());
    assert!(prefix("something_github_added_later").is_none());
}
