//! Reading a pull request's check rollup as one merge answer.
//!
//! The rollup is a flat list of runs, and read entry by entry it has two shapes that
//! can never go all-green: a terminal conclusion that is neither success nor failure,
//! and several runs of the same check where an older one was superseded. Both are
//! resolved here by judging the rollup per check *name*, on that name's most recent
//! run — which is the unit branch protection requires, and the one GitHub itself
//! answers a merge with.

use std::collections::HashMap;

use serde_json::Value;

/// Conclusions that mean a check finished and did not pass. Anything else — a null
/// conclusion, `PENDING`, `QUEUED`, `IN_PROGRESS` — is a check still in flight.
const FAILED_CONCLUSIONS: [&str; 7] = [
    "FAILURE",
    "TIMED_OUT",
    "CANCELLED",
    "ACTION_REQUIRED",
    "STARTUP_FAILURE",
    "STALE",
    "ERROR",
];

/// Conclusions that mean a check finished without standing in the way of a merge.
///
/// `SKIPPED` and `NEUTRAL` are as terminal as `SUCCESS`: a required workflow whose
/// path filter excluded the change reports one of them and will never report anything
/// else. Reading them as still-running parks such a pull request for ever.
const PASSED_CONCLUSIONS: [&str; 3] = ["SUCCESS", "SKIPPED", "NEUTRAL"];

/// What the check rollup says about merging right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Checks {
    /// The pull request is open and every check it reports is satisfied.
    Green,
    /// A check finished and did not pass: the worker's own head is red, which is
    /// something it can fix by pushing to the same branch.
    Failed,
    /// Not green, but nothing has failed either: the checks are still running.
    /// That does not change when a worker pushes.
    ///
    /// A pull request that is no longer open reads as `Waiting` too, but the gate
    /// no longer reaches this: [`super::pull_request::conclusion`] decides such a
    /// pull request first. The guard stays as a safety rail, so a state this
    /// classifier does not recognise still waits rather than merging on a stale
    /// rollup.
    Waiting,
}

/// What one run of one check says.
///
/// The ordering is the tie-break between two runs of the same check that started at
/// the same moment — duplicate triggers of one workflow do — and it deliberately
/// ranks the more cautious answer higher: a run still in flight outranks one that
/// passed, which outranks one that failed. Simultaneous runs therefore hold the gate
/// rather than let either half of the pair decide the merge alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Run {
    /// It finished and did not pass.
    Failed,
    /// It finished without standing in the way of a merge.
    Passed,
    /// It has not finished.
    Running,
}

/// Which check a rollup entry reports on.
///
/// A name is the unit branch protection speaks in: a ruleset requires the context
/// `validate / Validate`, and every run carrying that name is a run of the same
/// check. An entry GitHub reported without a name cannot be matched to any other, so
/// it stands alone under its position in the rollup.
#[derive(Debug, PartialEq, Eq, Hash)]
enum Check {
    Named(String),
    Unnamed(usize),
}

/// Reduces the rollup to one (name, run) per check — its most recent run, so a
/// superseded run cannot veto the re-run that replaced it: a `CANCELLED` half of a
/// duplicated workflow trigger is a fact about a run GitHub abandoned, not about the
/// head. Shared between [`classify`] and [`failed_names`] so both read the same
/// per-check verdict.
fn latest_runs(rollup: &[Value]) -> HashMap<Check, (String, Run)> {
    let mut latest: HashMap<Check, (String, Run)> = HashMap::new();
    for (position, entry) in rollup.iter().enumerate() {
        let candidate = (started_at(entry).to_owned(), run(entry));
        let held = latest
            .entry(check(entry, position))
            .or_insert_with(|| candidate.clone());
        if candidate > *held {
            *held = candidate;
        }
    }
    latest
}

/// Classifies the rollup of an open pull request.
///
/// A name whose newest run is a genuine failure still fails, because that run is the
/// one the branch protection reads too.
pub(crate) fn classify(rollup: &[Value]) -> Checks {
    let latest = latest_runs(rollup);
    // A head whose checks have not been created yet is not green.
    if latest.is_empty() {
        return Checks::Waiting;
    }
    if latest.values().any(|(_, run)| *run == Run::Failed) {
        return Checks::Failed;
    }
    if latest.values().any(|(_, run)| *run == Run::Running) {
        return Checks::Waiting;
    }
    Checks::Green
}

/// Names of the checks whose most recent run failed, sorted for a stable
/// display order — an Inbox row shows these verbatim (dropr:461), so an
/// operator sees the same check name branch protection would have blocked on.
pub(crate) fn failed_names(rollup: &[Value]) -> Vec<String> {
    let mut names: Vec<String> = latest_runs(rollup)
        .into_iter()
        .filter(|(_, (_, run))| *run == Run::Failed)
        .filter_map(|(check, _)| match check {
            Check::Named(name) => Some(name),
            Check::Unnamed(_) => None,
        })
        .collect();
    names.sort_unstable();
    names
}

/// The check the entry reports on: `name` is a check run's, `context` a commit
/// status's.
fn check(entry: &Value, position: usize) -> Check {
    text(entry, ["name", "context"])
        .map(|name| Check::Named(name.to_owned()))
        .unwrap_or(Check::Unnamed(position))
}

/// When the run began, which orders two runs of the same check.
///
/// GitHub reports fixed-width UTC timestamps, so comparing them as text is comparing
/// them as instants. An entry reporting none sorts oldest: a run that never started
/// has produced no evidence about the head, and letting one outrank a run that
/// actually reported is how a stranded queued duplicate holds a merge GitHub would
/// accept.
fn started_at(entry: &Value) -> &str {
    text(entry, ["startedAt", "createdAt"]).unwrap_or_default()
}

/// What the entry's own conclusion says about that one run.
fn run(entry: &Value) -> Run {
    let state = text(entry, ["conclusion", "state"]).unwrap_or_default();
    if PASSED_CONCLUSIONS.contains(&state) {
        Run::Passed
    } else if FAILED_CONCLUSIONS.contains(&state) {
        Run::Failed
    } else {
        Run::Running
    }
}

/// The first of `fields` the entry reports as a non-empty string.
fn text<'a>(entry: &'a Value, fields: [&str; 2]) -> Option<&'a str> {
    fields.into_iter().find_map(|field| {
        entry
            .get(field)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
    })
}

#[cfg(test)]
#[path = "check_rollup_tests.rs"]
mod tests;
