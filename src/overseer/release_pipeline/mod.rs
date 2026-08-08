//! Runs this project's own local release pipeline (`scripts/release.sh`)
//! after a merge closes a `[release]`-scoped task in this project's own
//! repository.
//!
//! Version-bump pull requests merge automatically, but publishing was a
//! separate manual step nobody remembered to run: several merged bumps sat
//! unreleased for days while the daemon kept running the old binary. This
//! module is what closes that gap, without turning into a second thing that
//! silently ships when it should not — every guard below fails closed:
//!
//! - `config.overseer.release_pipeline_enabled` must be on. This capability
//!   is a distinct privilege class from the rest of the daemon's autonomy:
//!   it runs a local shell script for up to thirty minutes and, on success,
//!   publishes a public GitHub release with whatever credentials the daemon
//!   holds — and `scripts/release.sh` is itself part of this repository, so
//!   a future change to it runs with this same privilege on the next
//!   qualifying merge. Default-off; an operator opts in deliberately. See
//!   `overseer::config::OverseerConfig::release_pipeline_enabled`.
//! - Only a repository that is a checkout of this very project (its
//!   `Cargo.toml` package name matches the running binary's own, and it
//!   carries `scripts/release.sh`) is ever considered. See [`is_self`].
//! - Only a pull request whose title carries this project's `[release]`
//!   task-scope prefix triggers anything. See [`RELEASE_SCOPE`].
//! - The repository's own checkout — never any worker's worktree, never
//!   pulled or touched by this module — must already be clean and already
//!   sitting on the commit `origin/main` just advanced to. Nothing here
//!   fast-forwards it: the same invariant `git::post_merge` keeps for every
//!   other post-merge step applies here too. See [`ready`].
//!
//! A repository or task that fails an early guard produces no log entry at
//! all — the common case, an ordinary merge in an ordinary repository, must
//! stay silent. Once a candidate clears the scope guards, every remaining
//! outcome (skipped for an unready checkout, published, or failed) is
//! recorded through the daemon's own decision log and, for the skip and the
//! two terminal outcomes alike, reaches Discord — see [`skip`] and [`run`] —
//! so an operator can always see why the last release did or did not ship,
//! promptly rather than by noticing an unpublished version days later.
//!
//! `scripts/release.sh` itself never leaves the checkout detached: its
//! `tag` stage creates the release tag with `git tag -a <tag> origin/main`,
//! which tags a commit without checking it out, and no other stage runs
//! `git checkout`. A checkout found detached at exactly the commit a prior
//! release published from is therefore evidence of an operator's own manual
//! step (for example checking out the tag to verify the built artifact),
//! not of this pipeline — see task dropr:8uAzfpolZ3OupBaaq47hD. That is also
//! why [`ready`] only ever reports and never repairs: recovering from a
//! manual checkout would mean this module writing to a checkout that isn't
//! its own, which is the one thing every guard above exists to avoid.

use std::{
    fs,
    path::Path,
    process::{Command, Output},
    time::Duration,
};

use serde_json::Value;

use crate::{
    Result,
    exec::run_timeout,
    overseer::{
        exec::COMMAND_TIMEOUT,
        logging::{self, DecisionEntry, DecisionKind},
    },
};

mod readiness;
use readiness::ready;

/// Task-title scope prefix for the version-bump tasks this pipeline covers,
/// matching the `emoji [scope] description` convention this project's own
/// dropr workspace rules document.
const RELEASE_SCOPE: &str = "[release]";

const SCRIPT: &str = "scripts/release.sh";

/// How long `scripts/release.sh` may run before the daemon gives up on it.
/// The pipeline cross-compiles four targets and publishes to two remote
/// repositories — comfortably the longest-running command this daemon ever
/// shells out to — so it gets its own bound well past every other timeout in
/// this codebase, rather than sharing one sized for a `gh`/`git` round trip.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Considers whether the pull request that just merged `repo` onto `main`
/// should trigger the local release pipeline, and runs it when every guard
/// clears.
///
/// Called once per merge from `overseer::exec::execute_actions`, never from
/// `overseer::monitor`: the reconcile pass that schedules this stays a pure
/// diff over ledger snapshots, and everything here — the repository read,
/// the pull request read, and the pipeline run itself — belongs with the
/// daemon's other side-effecting steps.
///
/// `enabled` is `config.overseer.release_pipeline_enabled`, checked first
/// and unconditionally: every guard after this one narrows *which* merge
/// qualifies, but this one is the operator's own decision to grant the
/// pipeline's privilege class at all, and it must not be reachable by
/// satisfying the narrower guards alone.
pub(crate) fn consider(
    task_id: &str,
    repo: &str,
    pr_url: Option<&str>,
    enabled: bool,
) -> Result<()> {
    if !enabled {
        return Ok(());
    }
    let repo_path = Path::new(repo);
    if !repo_path.join(SCRIPT).exists() || !is_self(repo_path) {
        return Ok(());
    }
    let Some(pr_url) = pr_url else {
        return Ok(());
    };
    let Some(title) = pr_title(repo, pr_url) else {
        return Ok(());
    };
    if !title.contains(RELEASE_SCOPE) {
        return Ok(());
    }
    match ready(repo_path) {
        Ok(()) => run(task_id, repo, pr_url, repo_path),
        Err(reason) => skip(task_id, repo, pr_url, reason),
    }
}

/// Whether `repo` is a checkout of this same project, judged by comparing
/// its `Cargo.toml` package name against the name this very binary was
/// built from. A managed repository that happens to also ship a
/// `scripts/release.sh` of its own must not have this project's release
/// steps run against it — the script-existence check alone cannot tell the
/// two apart, so this is the guard that actually does.
fn is_self(repo: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(repo.join("Cargo.toml")) else {
        return false;
    };
    contents
        .lines()
        .find(|line| line.starts_with("name"))
        .is_some_and(|line| line.contains(&format!("\"{}\"", env!("CARGO_PKG_NAME"))))
}

fn pr_title(repo: &str, url: &str) -> Option<String> {
    let mut view = Command::new("gh");
    view.current_dir(repo)
        .args(["pr", "view", url, "--json", "title"]);
    let output = run_timeout(view, COMMAND_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice::<Value>(&output.stdout)
        .ok()?
        .get("title")?
        .as_str()
        .map(str::to_owned)
}

fn run(task_id: &str, repo: &str, pr_url: &str, repo_path: &Path) -> Result<()> {
    let mut command = Command::new("bash");
    command.current_dir(repo_path).arg(SCRIPT);
    match run_timeout(command, RELEASE_TIMEOUT) {
        Ok(output) if output.status.success() => {
            let version = crate_version(repo_path).unwrap_or_else(|| "unknown".into());
            report(
                task_id,
                repo,
                pr_url,
                DecisionKind::Release,
                "daemon_event",
                format!(
                    "release_published:Published RobCo {version} (tag v{version}). The \
                     daemon is still running the build it started with — restart it to \
                     pick up this and any other merged fix."
                ),
            )
        }
        Ok(output) => report(
            task_id,
            repo,
            pr_url,
            DecisionKind::Release,
            "daemon_event",
            format!("release_failed:Failed at {}.", failure_excerpt(&output)),
        ),
        Err(error) => report(
            task_id,
            repo,
            pr_url,
            DecisionKind::Release,
            "daemon_event",
            format!("release_failed:{SCRIPT} did not run: {error}."),
        ),
    }
}

/// Records the skip and, by using the `daemon_event` source `run`'s own
/// outcomes use (`overseer::discord::notifications::from_decision` keys its
/// `release_pipeline_skipped:` match arm on that source), makes it reach
/// Discord like a published or failed release does — an unready checkout
/// blocks every release after it, so this is deliberately louder than an
/// ordinary `daemon`-sourced skip.
fn skip(task_id: &str, repo: &str, pr_url: &str, reason: &'static str) -> Result<()> {
    report(
        task_id,
        repo,
        pr_url,
        DecisionKind::Skip,
        "daemon_event",
        format!("release_pipeline_skipped:{reason}"),
    )
}

fn report(
    task_id: &str,
    repo: &str,
    pr_url: &str,
    kind: DecisionKind,
    source: &str,
    reason: String,
) -> Result<()> {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(task_id.to_owned());
    entry.repo = Some(repo.to_owned());
    entry.pr_url = Some(pr_url.to_owned());
    entry.source = Some(source.to_owned());
    logging::append(&entry)
}

/// The stage `scripts/release.sh` was on when it exited non-zero, and its
/// own explanation, pulled from the output the daemon already captured
/// rather than re-running anything. The script names each stage as it
/// starts (`log "stage: <name>"`, to stdout) and names the fatal condition
/// once (`die`'s `[release] error: <message>`, to stderr); the last of each
/// is what was running, and why, when the pipeline stopped.
fn failure_excerpt(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stage = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("[release] stage: "))
        .next_back()
        .unwrap_or("unknown");
    let error = stderr
        .lines()
        .rev()
        .find(|line| line.starts_with("[release] error:"))
        .or_else(|| stderr.lines().last())
        .unwrap_or("no output")
        .trim();
    format!("stage={stage} {error}")
}

/// `Cargo.toml`'s own `version`, read the same way `scripts/release.sh`
/// itself does (`grep '^version' Cargo.toml | head -1`) so the reported
/// number always matches what the script just published.
fn crate_version(repo: &Path) -> Option<String> {
    let contents = fs::read_to_string(repo.join("Cargo.toml")).ok()?;
    let line = contents.lines().find(|line| line.starts_with("version"))?;
    let start = line.find('"')? + 1;
    let end = start + line[start..].find('"')?;
    Some(line[start..end].to_owned())
}

#[cfg(test)]
#[path = "release_pipeline_tests.rs"]
mod tests;
