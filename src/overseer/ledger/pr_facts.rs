//! What the daemon last read about a pull request's own case.
//!
//! `Remedy`'s guidance is `&'static str` — a category's advice, never this
//! pull request's. This is the other half: the per-case facts an Inbox row
//! needs to say what is actually waiting, captured once per gate pass from
//! the same `gh pr view` payload the gate already reads
//! (`daemon::pr_facts::extract`) rather than a fresh call from the render
//! path. Overwritten every successful read, so a stale value only lingers
//! across a read failure, never past the next one that succeeds.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PrFacts {
    pub title: String,
    pub files_changed: u32,
    pub lines_changed: u32,
    /// Checks whose most recent run failed, empty when none has. Distinct
    /// from an absent `PrFacts` altogether: a pull request can be known and
    /// green.
    pub failed_checks: Vec<String>,
}
