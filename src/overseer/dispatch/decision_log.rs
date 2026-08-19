//! Decision-log emission for a named launch (`!run <task>`). Every skip and
//! hold `resolve::resolve_task` and `worker::spawn_candidate` record into
//! `decisions.jsonl` is shaped here. The one policy decision that exists only
//! to gate its own log line — [`skip_unmaterialised`] — lives here with the
//! entry it writes.

use std::collections::BTreeSet;

use super::Candidate;
use crate::overseer::logging::{self, DecisionEntry, DecisionKind};
use crate::{Result, dropr};

pub(super) fn log_candidate(kind: DecisionKind, task: &Candidate, reason: &str) -> Result<()> {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(task.task_id.clone());
    entry.repo = Some(task.repo.clone());
    entry.source = Some("dispatch".into());
    logging::append(&entry)
}

pub(super) fn log_repo_skip<F>(repo: &str, reason: &str, append: F) -> Result<()>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    let mut entry = DecisionEntry::new(DecisionKind::Skip, reason);
    entry.repo = Some(repo.into());
    entry.source = Some("dispatch".into());
    append(&entry)
}

pub(super) fn log_ready_failure<F>(
    repo: &str,
    workspace: &str,
    error: dropr::ReadyDispatchError,
    append: F,
) -> Result<()>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    let mut entry = DecisionEntry::new(DecisionKind::Skip, error.reason());
    entry.repo = Some(repo.into());
    entry.source = Some("dispatch".into());
    entry.reason = format!("{}:{workspace}", error.reason());
    append(&entry)
}

/// Records a global (not candidate-scoped) decision at most once while the
/// same reason holds — currently just `dropr_overlay_unavailable` from
/// `resolve::resolve_task`. `logged` is owned by the daemon loop and cleared
/// once a resolution gets past the overlay read, so a later recurrence logs
/// fresh.
pub(super) fn log_global_once<F>(
    kind: DecisionKind,
    reason: &str,
    logged: &mut Option<String>,
    append: F,
) -> Result<()>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    if logged.as_deref() == Some(reason) {
        return Ok(());
    }
    *logged = Some(reason.to_owned());
    let mut entry = DecisionEntry::new(kind, reason);
    entry.source = Some("dispatch".into());
    append(&entry)
}

/// Decides whether a repo's workspace can serve a ready feed at all, and
/// returns `true` when the fetch must be skipped.
///
/// A virtual (never-materialised) workspace has no task board behind it, so
/// `dropr task ready` answers HTTP 404 on every tick — a steady state, not a
/// transient fault. Retrying it each pass would spend the fetch budget and
/// write one `ready_fetch_failed` decision per minute, all day
/// (dropr:rC8ZxtZT913zsmYfnOFhs). Instead the skip is recorded once per daemon
/// run (`logged` is owned by the daemon loop) and later ticks stay quiet.
///
/// The workspace overlay is reloaded on every pass, so a workspace that gets
/// materialised later flips this check on the next tick without a daemon
/// restart; the repo is then dropped from `logged` so a workspace that ever
/// reverts to virtual logs one fresh decision rather than none.
pub(super) fn skip_unmaterialised<F>(
    repo: &str,
    workspace: &dropr::DroprWorkspace,
    logged: &mut BTreeSet<String>,
    append: F,
) -> Result<bool>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    if workspace.is_materialised() {
        logged.remove(repo);
        return Ok(false);
    }
    if logged.insert(repo.to_string()) {
        log_repo_skip(repo, "workspace_not_materialised", append)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(kind: &str) -> dropr::DroprWorkspace {
        dropr::DroprWorkspace {
            kind: kind.into(),
            id: "github:owner/repo".into(),
            name: "repo".into(),
            repo_url: "https://github.com/owner/repo".into(),
        }
    }

    #[test]
    fn a_repeated_global_reason_writes_once_and_a_changed_one_writes_again() {
        let mut logged = None;
        let mut reasons = Vec::new();
        for reason in ["dispatch_disabled", "dispatch_disabled", "daily_limit"] {
            log_global_once(DecisionKind::Skip, reason, &mut logged, |e| {
                reasons.push(e.reason.clone());
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(reasons, ["dispatch_disabled", "daily_limit"]);
        assert_eq!(logged.as_deref(), Some("daily_limit"));
    }

    #[test]
    fn an_unmaterialised_workspace_is_skipped_and_logged_once_per_run() {
        // The dropr:rC8ZxtZT913zsmYfnOFhs loop: a virtual workspace 404s on every
        // ready fetch, so the repo must be skipped before fetching, with exactly
        // one decision entry for the whole daemon run rather than one per tick.
        let mut logged = BTreeSet::new();
        let mut captured = None;
        let skipped = skip_unmaterialised("/repo", &workspace("virtual"), &mut logged, |entry| {
            captured = Some(entry.clone());
            Ok(())
        })
        .unwrap();
        assert!(skipped);
        let entry = captured.unwrap();
        assert_eq!(entry.kind, DecisionKind::Skip);
        assert_eq!(entry.reason, "workspace_not_materialised");
        assert_eq!(entry.repo.as_deref(), Some("/repo"));

        // Every later tick: still skipped, but silently.
        for _ in 0..3 {
            let skipped = skip_unmaterialised("/repo", &workspace("virtual"), &mut logged, |_| {
                panic!("a repeated tick must not log again")
            })
            .unwrap();
            assert!(skipped);
        }
    }

    #[test]
    fn materialising_the_workspace_resumes_dispatch_and_rearms_the_log() {
        // The overlay reloads every pass, so the flip to `materialised` must be
        // enough on its own — no daemon restart — and a workspace that later
        // reverts to virtual gets one fresh decision, not silence.
        let mut logged = BTreeSet::from(["/repo".to_string()]);
        let skipped = skip_unmaterialised("/repo", &workspace("materialised"), &mut logged, |_| {
            panic!("a materialised workspace must not log a skip")
        })
        .unwrap();
        assert!(!skipped);

        let mut captured = None;
        let skipped = skip_unmaterialised("/repo", &workspace("virtual"), &mut logged, |entry| {
            captured = Some(entry.clone());
            Ok(())
        })
        .unwrap();
        assert!(skipped);
        assert_eq!(captured.unwrap().reason, "workspace_not_materialised");
    }

    #[test]
    fn repo_skip_emits_skip_decision() {
        let mut captured = None;
        log_repo_skip("/repo", "repo_path_missing", |entry| {
            captured = Some(entry.clone());
            Ok(())
        })
        .unwrap();
        let entry = captured.unwrap();
        assert_eq!(entry.kind, DecisionKind::Skip);
        assert_eq!(entry.reason, "repo_path_missing");
        assert_eq!(entry.repo.as_deref(), Some("/repo"));
    }

    #[test]
    fn fetch_failure_emits_skip_decision() {
        let mut captured = None;
        log_ready_failure(
            "/repo",
            "workspace-1",
            dropr::ReadyDispatchError::Parse,
            |entry| {
                captured = Some(entry.clone());
                Ok(())
            },
        )
        .unwrap();
        let entry = captured.unwrap();
        assert_eq!(entry.kind, DecisionKind::Skip);
        assert_eq!(entry.reason, "ready_parse_failed:workspace-1");
        assert_eq!(entry.repo.as_deref(), Some("/repo"));
    }
}
