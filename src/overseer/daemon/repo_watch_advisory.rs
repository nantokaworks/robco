//! Security-advisory drift: `bun audit` (when the checkout has a `bun.lock`)
//! and `govulncheck ./...` (when it has a `go.mod`), run against a
//! repository's local checkout on `repo_watch`'s cadence. A failing run that
//! names at least one advisory id files one coordination task; a failing run
//! that names none (network trouble) is logged and skipped rather than
//! treated as an advisory (dropr task #430). A probe whose binary is not
//! installed is a separate, permanent host fact rather than a per-run
//! failure — it is reported once, not on every due repository forever
//! (dropr task #445).

use std::{collections::BTreeSet, path::Path, process::Command, time::Duration};

use super::repo_watch_task;
use crate::overseer::{exec::run_timeout, logging};

/// `govulncheck` walks a module graph and can run considerably longer than
/// the 15s `COMMAND_TIMEOUT` daemon.rs uses for `gh` calls — the task's own
/// estimate is "~20s per repo".
const PROBE_TIMEOUT: Duration = Duration::from_secs(120);

struct Finding {
    ids: Vec<String>,
    output: String,
}

/// Why a probe did not produce a [`Finding`].
enum ProbeError {
    /// The probe's binary is not on `PATH` — a permanent host fact, not a
    /// failure of this run.
    Missing(&'static str),
    /// The probe ran and failed for a reason that named no advisory id
    /// (network trouble, a tool crash).
    Failed(String),
}

/// Probes one repository and files a task when advisories are found.
/// Best-effort per probe: a `bun audit` failure does not stop `govulncheck`
/// from running. A tool-level failure is returned rather than swallowed, so
/// the caller can log it; a missing tool is instead reported once via
/// `reported_missing_tools` and otherwise treated as the probe not applying
/// here, the same as a repository without the matching lockfile.
pub(super) fn run(
    repo_path: &Path,
    workspace_id: &str,
    repo_name: &str,
    reported_missing_tools: &mut BTreeSet<String>,
) -> Result<(), String> {
    let mut problems = Vec::new();
    let bun = match probe_bun_audit(repo_path) {
        Ok(finding) => finding,
        Err(ProbeError::Missing(label)) => {
            note_missing_tool(label, reported_missing_tools)?;
            None
        }
        Err(ProbeError::Failed(problem)) => {
            problems.push(problem);
            None
        }
    };
    let go = match probe_govulncheck(repo_path) {
        Ok(finding) => finding,
        Err(ProbeError::Missing(label)) => {
            note_missing_tool(label, reported_missing_tools)?;
            None
        }
        Err(ProbeError::Failed(problem)) => {
            problems.push(problem);
            None
        }
    };
    file_if_found(workspace_id, repo_name, bun, go)?;
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

/// Logs a missing-tool setup notice the first time `label` is seen, and stays
/// quiet on every later call — the fact does not change until an operator
/// installs the tool, so repeating it every due repository would just be
/// noise.
fn note_missing_tool(
    label: &str,
    reported_missing_tools: &mut BTreeSet<String>,
) -> Result<(), String> {
    if !reported_missing_tools.insert(label.to_string()) {
        return Ok(());
    }
    logging::log_message(
        None,
        &format!("{label} is not installed; its advisory probe is skipped until it is available"),
    )
    .map_err(|error| format!("decision log write failed: {error}"))
}

/// Combines whatever the two probes found, and files one task when the
/// merged id set is non-empty and no equivalent task is already open.
fn file_if_found(
    workspace_id: &str,
    repo_name: &str,
    bun: Option<Finding>,
    go: Option<Finding>,
) -> Result<(), String> {
    let mut ids: Vec<String> = Vec::new();
    if let Some(finding) = &bun {
        ids.extend(finding.ids.iter().cloned());
    }
    if let Some(finding) = &go {
        ids.extend(finding.ids.iter().cloned());
    }
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Ok(());
    }
    let marker = marker(repo_name, &ids);
    match repo_watch_task::already_open(workspace_id, &marker) {
        None | Some(true) => Ok(()),
        Some(false) => {
            let title = title(repo_name, ids.len(), &marker);
            let body = body(repo_name, bun.as_ref(), go.as_ref(), &marker);
            repo_watch_task::file(
                workspace_id,
                repo_name,
                &title,
                &body,
                "advisory_task_filed",
            )
        }
    }
}

fn probe_bun_audit(repo_path: &Path) -> Result<Option<Finding>, ProbeError> {
    if !repo_path.join("bun.lock").exists() && !repo_path.join("bun.lockb").exists() {
        return Ok(None);
    }
    let mut command = Command::new("bun");
    command.current_dir(repo_path).arg("audit");
    probe(command, "bun", "bun audit")
}

fn probe_govulncheck(repo_path: &Path) -> Result<Option<Finding>, ProbeError> {
    if !repo_path.join("go.mod").exists() {
        return Ok(None);
    }
    let mut command = Command::new("govulncheck");
    command.current_dir(repo_path).arg("./...");
    probe(command, "govulncheck", "govulncheck")
}

/// Runs one advisory tool and reads its result. A clean exit is `Ok(None)`,
/// same as the tool not applying to this repository. The tool's binary being
/// absent from `PATH` (`io::ErrorKind::NotFound` from spawning it) is
/// [`ProbeError::Missing`] rather than a failure. A non-zero exit with no
/// advisory id anywhere in its combined output is [`ProbeError::Failed`] — a
/// tool crash or a network failure, not an advisory — so the caller logs it
/// instead of filing a task that names nothing.
fn probe(command: Command, tool: &'static str, label: &str) -> Result<Option<Finding>, ProbeError> {
    let output = run_timeout(command, PROBE_TIMEOUT).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProbeError::Missing(tool)
        } else {
            ProbeError::Failed(format!("{label}: {error}"))
        }
    })?;
    if output.status.success() {
        return Ok(None);
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let ids = advisory_ids(&text);
    if ids.is_empty() {
        return Err(ProbeError::Failed(format!(
            "{label} exited {}, no advisory id found",
            output.status
        )));
    }
    Ok(Some(Finding { ids, output: text }))
}

/// Advisory ids (`GHSA-xxxx-xxxx-xxxx`, `GO-YYYY-NNNN`) found anywhere in
/// `text`, sorted and deduped. No `regex` dependency: tokens are split on
/// every byte that cannot appear inside an id, the same manual-parse style
/// `daemon::external_prs::closes_task` already uses in this codebase.
fn advisory_ids(text: &str) -> Vec<String> {
    let mut ids: Vec<String> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|token| is_ghsa_id(token) || is_go_id(token))
        .map(str::to_string)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn is_ghsa_id(token: &str) -> bool {
    let Some(rest) = token.strip_prefix("GHSA-") else {
        return false;
    };
    let groups: Vec<&str> = rest.split('-').collect();
    groups.len() == 3
        && groups
            .iter()
            .all(|group| group.len() >= 4 && group.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn is_go_id(token: &str) -> bool {
    let Some(rest) = token.strip_prefix("GO-") else {
        return false;
    };
    let Some((year, number)) = rest.split_once('-') else {
        return false;
    };
    year.len() == 4
        && year.chars().all(|c| c.is_ascii_digit())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

/// Stable per-condition key: the caller re-checks the board for a title
/// already containing this exact string before filing again. Sorted ids make
/// the marker order-independent; a changed id set is, by design, a new
/// condition and gets its own task.
fn marker(repo_name: &str, ids: &[String]) -> String {
    format!("[repo-watch:advisory:{repo_name}:{}]", ids.join(","))
}

fn title(repo_name: &str, count: usize, marker: &str) -> String {
    format!(
        "✨ [overseer] Security advisories are failing CI checks in {repo_name} ({count} found) {marker}"
    )
}

fn body(repo_name: &str, bun: Option<&Finding>, go: Option<&Finding>, marker: &str) -> String {
    let mut sections = vec![format!(
        "## Why\nOverseer's repository health watch found failing security advisories on \
         {repo_name}'s main branch. See dropr task #430 for the watch itself.\n"
    )];
    if let Some(finding) = bun {
        sections.push(format!(
            "## bun audit\nIds found: {}\n\n```\n{}\n```\n",
            finding.ids.join(", "),
            clip(&finding.output)
        ));
    }
    if let Some(finding) = go {
        sections.push(format!(
            "## govulncheck ./...\nIds found: {}\n\n```\n{}\n```\n",
            finding.ids.join(", "),
            clip(&finding.output)
        ));
    }
    sections.push(format!(
        "## Dedup marker\nDo not remove `{marker}` from the title — Overseer's repo watch \
         uses it to avoid filing a duplicate task while this one stays open."
    ));
    sections.join("\n")
}

/// Caps one probe's raw output embedded in a task body — a task description
/// is for triage, not for holding an unbounded audit log.
fn clip(output: &str) -> String {
    const LIMIT: usize = 4000;
    if output.chars().count() <= LIMIT {
        return output.trim().to_string();
    }
    let clipped: String = output.chars().take(LIMIT).collect();
    format!("{}\n… (truncated)", clipped.trim())
}

#[cfg(test)]
#[path = "repo_watch_advisory_tests.rs"]
mod tests;
