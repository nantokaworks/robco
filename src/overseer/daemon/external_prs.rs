//! Discovers pull requests Overseer did not dispatch — Dependabot bumps, a
//! human's own branch, another agent's — so the operator can see them without
//! RobCo pretending to have driven them (dropr task #350).
//!
//! Kept apart from `external_state`'s probes: those ask about one pull
//! request a ledger entry already names, while this one lists a repository's
//! open pull requests wholesale and keeps only the ones no entry claims.

use std::{path::Path, process::Command};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::COMMAND_TIMEOUT;
use crate::{
    Result,
    overseer::{
        exec::run_timeout,
        ledger::Ledger,
        logging,
        other_prs::{OtherPr, OtherPrs, RepoOtherPrs},
    },
    registry::Registry,
};

/// How long a repository's discovered pull-request list is trusted before
/// this probe asks GitHub again.
///
/// The daemon tick runs every `poll_interval_secs` (60s by default); listing
/// every watched repository's open pull requests on every tick would cost
/// `repos * 1440` `gh` calls a day for no benefit an operator can perceive —
/// four watched repositories at that cadence is 5,760 calls a day before
/// anything else runs. A Dependabot pull request sitting a few extra minutes
/// unlisted is not the failure mode task #350 exists to fix (nex's #742 sat
/// invisible for twelve hours). Five minutes bounds the added cost to
/// `repos * 288` calls a day while keeping the list fresh enough to matter.
const REFRESH_INTERVAL: chrono::Duration = chrono::Duration::minutes(5);

const PR_FIELDS: &str = "number,title,author,url,headRefName,mergeStateStatus";

#[derive(Deserialize)]
struct RawPr {
    number: u64,
    title: String,
    author: RawAuthor,
    url: String,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: String,
}

#[derive(Deserialize)]
struct RawAuthor {
    login: String,
}

/// Refreshes the pull requests Overseer did not dispatch, one watched
/// repository at a time, backing off per repository per [`REFRESH_INTERVAL`].
///
/// Best-effort: a repository whose `gh` call fails keeps its last known list
/// rather than going blank, the same tolerance `external_state` gives every
/// other GitHub read. Registry and cache loads are the only failures this
/// propagates, since without either there is nothing to refresh or nowhere
/// to save the result.
pub(super) fn refresh_pass(ledger: &Ledger, now: DateTime<Utc>) -> Result<()> {
    let registry = Registry::load()?;
    let mut state = OtherPrs::load()?;
    for repo in &registry.repos {
        let key = repo.path.to_string_lossy().into_owned();
        if !is_due(state.repos.get(&key), now) {
            continue;
        }
        match list_open_prs(&repo.path) {
            Ok(raw_prs) => {
                let prs = raw_prs
                    .into_iter()
                    .filter(|pr| !dispatched_by_overseer(ledger, &key, &pr.head_ref_name))
                    .map(|pr| OtherPr {
                        number: pr.number,
                        title: pr.title,
                        author: pr.author.login,
                        url: pr.url,
                        head_ref_name: pr.head_ref_name,
                        mergeable_state: pr.merge_state_status,
                    })
                    .collect();
                state.repos.insert(
                    key,
                    RepoOtherPrs {
                        polled_at: now,
                        prs,
                    },
                );
            }
            Err(error) => logging::log_message(
                None,
                &format!("other-PR list skipped for {}: {error}", repo.path.display()),
            )?,
        }
    }
    state.save()
}

/// Whether a repository's cached pull-request list is old enough to re-ask
/// GitHub about — absent entirely counts as due, the same as an entry older
/// than [`REFRESH_INTERVAL`].
fn is_due(cached: Option<&RepoOtherPrs>, now: DateTime<Utc>) -> bool {
    cached.is_none_or(|entry| now.signed_duration_since(entry.polled_at) >= REFRESH_INTERVAL)
}

/// Whether Overseer itself dispatched the branch a pull request sits on —
/// live or settled, since a settled entry already surfaces in the HISTORY
/// section and must not also appear here as if it arrived on its own.
fn dispatched_by_overseer(ledger: &Ledger, repo: &str, head_ref_name: &str) -> bool {
    ledger
        .entries
        .iter()
        .any(|entry| entry.repo == repo && entry.branch == head_ref_name)
}

fn list_open_prs(repo: &Path) -> std::result::Result<Vec<RawPr>, String> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo)
        .args(["pr", "list", "--state", "open", "--json", PR_FIELDS]);
    let output = match run_timeout(command, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => output,
        Ok(output) => return Err(format!("gh pr list exited {}", output.status)),
        Err(error) => return Err(format!("gh pr list skipped: {error}")),
    };
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("gh pr list JSON unreadable: {error}"))
}

#[cfg(test)]
#[path = "external_prs_tests.rs"]
mod tests;
