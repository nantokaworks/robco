//! Dependabot coordination: a conflicted (`DIRTY`) or stale-by-age Dependabot
//! pull request needs a human or agent to work out the rebase order — nex's
//! `rebase-strategy: disabled` means the bot itself never resolves this. One
//! coordination task per repository names every qualifying pull request
//! (dropr task #430).

use std::{path::Path, process::Command};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{COMMAND_TIMEOUT, repo_watch_task};
use crate::overseer::exec::run_timeout;

const PR_FIELDS: &str = "number,title,url,author,mergeStateStatus,createdAt";

/// GitHub's login for pull requests Dependabot itself opens. `gh pr list
/// --json author` has answered both forms depending on `gh` version: the
/// classic bot-account form and the newer app-slug form. Neither has ever
/// been observed to include an "s" (`dependabot-preview[bot]` is a distinct,
/// unrelated legacy bot), so both are checked rather than picking one and
/// leaving the watch silently blind again on the next `gh` upgrade.
const DEPENDABOT_LOGINS: [&str; 2] = ["dependabot[bot]", "app/dependabot"];

#[derive(Debug, Clone, Deserialize)]
struct RawPr {
    number: u64,
    title: String,
    url: String,
    author: RawAuthor,
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: String,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawAuthor {
    login: String,
}

pub(super) fn run(
    repo_path: &Path,
    workspace_id: &str,
    repo_name: &str,
    stale_after_days: i64,
) -> Result<(), String> {
    let prs = list_dependabot_prs(repo_path)?;
    let stale = stale_prs(&prs, Utc::now(), stale_after_days);
    if stale.is_empty() {
        return Ok(());
    }
    let mut numbers: Vec<u64> = stale.iter().map(|pr| pr.number).collect();
    numbers.sort_unstable();
    let marker = marker(repo_name, &numbers);
    match repo_watch_task::already_open(workspace_id, &marker) {
        None | Some(true) => Ok(()),
        Some(false) => {
            let title = title(repo_name, stale.len(), &marker);
            let body = body(repo_name, &stale, stale_after_days, &marker);
            repo_watch_task::file(
                workspace_id,
                repo_name,
                &title,
                &body,
                "dependabot_task_filed",
            )
        }
    }
}

/// Pull requests needing coordinated attention: conflicted with their base
/// regardless of age, or open longer than `stale_after_days` even while
/// still clean — both are the steady-state failure task #430 exists to
/// surface, a chain of Dependabot bumps nothing ever rebases on its own.
fn stale_prs(prs: &[RawPr], now: DateTime<Utc>, stale_after_days: i64) -> Vec<&RawPr> {
    let threshold = chrono::Duration::days(stale_after_days);
    prs.iter()
        .filter(|pr| {
            pr.merge_state_status == "DIRTY"
                || now.signed_duration_since(pr.created_at) >= threshold
        })
        .collect()
}

fn marker(repo_name: &str, numbers: &[u64]) -> String {
    let joined = numbers
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("[repo-watch:dependabot:{repo_name}:{joined}]")
}

fn title(repo_name: &str, count: usize, marker: &str) -> String {
    format!(
        "✨ [overseer] {count} Dependabot pull request(s) need coordinated rebase in {repo_name} {marker}"
    )
}

fn body(repo_name: &str, prs: &[&RawPr], stale_after_days: i64, marker: &str) -> String {
    let mut lines = vec![format!(
        "## Why\nOverseer's repository health watch found Dependabot pull requests on \
         {repo_name} that are conflicted or have sat open past the {stale_after_days}-day \
         staleness threshold. {repo_name} disables Dependabot's own rebase-strategy, so \
         nothing resolves this without a coordinated rebase order. See dropr task #430 for \
         the watch itself.\n\n## Pull requests"
    )];
    for pr in prs {
        lines.push(format!(
            "- #{} [{}]({}) — {} — opened {}",
            pr.number, pr.title, pr.url, pr.merge_state_status, pr.created_at
        ));
    }
    lines.push(format!(
        "\n## Dedup marker\nDo not remove `{marker}` from the title — Overseer's repo watch \
         uses it to avoid filing a duplicate task while this one stays open."
    ));
    lines.join("\n")
}

fn list_dependabot_prs(repo_path: &Path) -> Result<Vec<RawPr>, String> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo_path)
        .args(["pr", "list", "--state", "open", "--json", PR_FIELDS]);
    let output =
        run_timeout(command, COMMAND_TIMEOUT).map_err(|error| format!("gh pr list: {error}"))?;
    if !output.status.success() {
        return Err(format!("gh pr list exited {}", output.status));
    }
    let prs: Vec<RawPr> = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("gh pr list JSON unreadable: {error}"))?;
    Ok(prs.into_iter().filter(|pr| is_dependabot(&pr.author.login)).collect())
}

fn is_dependabot(login: &str) -> bool {
    DEPENDABOT_LOGINS.contains(&login)
}

#[cfg(test)]
#[path = "repo_watch_dependabot_tests.rs"]
mod tests;
