//! The reasons this codebase actually writes, and what each one means.
//!
//! Split by domain — `table::merge` for the auto-merge gate, `table::runtime`
//! for dispatch, triage, and the review pass — once the combined table passed
//! the 300-line file limit. This file only combines the two and picks the
//! longest matching prefix, so a more specific entry never loses to a shorter
//! one that happens to also match.
//!
//! This table is deliberately not exhaustive. Anything not listed here falls
//! through to `super::classify_fallback`, which still answers correctly —
//! just with generic wording — for any reason `classify` already has an
//! opinion about, and defaults every other unknown reason to `Review`. A
//! table entry is worth adding only when the generic wording would be worse
//! than useless, or when `classify` has no opinion at all (`checks_waiting`,
//! the circuit family).

use super::Remedy;

mod merge;
mod runtime;

pub(super) fn exact(reason: &str) -> Option<Remedy> {
    merge::EXACT
        .iter()
        .chain(runtime::EXACT)
        .find(|(key, _)| *key == reason)
        .map(|(_, remedy)| *remedy)
}

pub(super) fn prefix(reason: &str) -> Option<Remedy> {
    merge::PREFIX
        .iter()
        .chain(runtime::PREFIX)
        .filter(|(key, _)| reason.starts_with(key))
        .max_by_key(|(key, _)| key.len())
        .map(|(_, remedy)| *remedy)
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
