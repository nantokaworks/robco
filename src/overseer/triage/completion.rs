use super::{
    ExceptionCase,
    actions::{execute_action, worker_session_alive},
    result::{self, Outcome, TriageAction},
};
use crate::{
    Result,
    overseer::{
        exec::COMMAND_TIMEOUT,
        ledger::{Ledger, LedgerPhase},
        logging::{self, DecisionEntry, DecisionKind},
        session::SessionResult,
    },
};
use chrono::Utc;
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

/// Reason seeded on a triage escalation's reconsideration budget — see
/// `LedgerEntry::grant_merge_reconsideration`. Never a gate reason itself,
/// so the merge pass's first re-read of the pull request always counts as a
/// change against it.
const TRIAGE_ESCALATION: &str = "triage_escalation";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Completion {
    outcome: Outcome,
    action: Option<TriageAction>,
    reason: String,
    /// Carried from `result::parse`'s doc comment: a schema mismatch or a
    /// policy rejection on `action` that `result::parse` already recovered
    /// from, dropping the action rather than the whole completion.
    /// `#[serde(default)]` so a marker written before this field existed
    /// still replays. `None` on every non-`Result` outcome — there is no
    /// action to have rejected.
    #[serde(default)]
    action_warning: Option<String>,
}

pub(super) fn complete_session_result(
    result: SessionResult,
    ledger: &mut Ledger,
    case: &ExceptionCase,
    case_dir: &Path,
    log_path: &Path,
) -> Result<()> {
    complete_session_result_with(
        result,
        ledger,
        case,
        case_dir,
        log_path,
        &execute_action,
        &dropr_scribble,
    )
}

fn complete_session_result_with(
    result: SessionResult,
    ledger: &mut Ledger,
    case: &ExceptionCase,
    case_dir: &Path,
    log_path: &Path,
    action: &dyn Fn(&TriageAction, &ExceptionCase) -> Result<()>,
    scribble: &dyn Fn(&str, &str, &str) -> crate::dropr::WriteResult,
) -> Result<()> {
    let marker = case_dir.join("outcome.json");
    let (completion, replay) = match fs::read(&marker) {
        Ok(raw) => (serde_json::from_slice(&raw)?, true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let completion = normalize(result, case);
            write_marker(&marker, &completion)?;
            (completion, false)
        }
        Err(error) => return Err(error.into()),
    };
    apply_completion(
        completion,
        replay,
        Some(&marker),
        ledger,
        case,
        log_path,
        action,
        scribble,
    )
}

#[cfg(test)]
pub(super) fn apply_session_result_with(
    result: SessionResult,
    ledger: &mut Ledger,
    case: &ExceptionCase,
    log_path: &Path,
    scribble: &dyn Fn(&str, &str, &str) -> crate::dropr::WriteResult,
) -> Result<()> {
    apply_completion(
        normalize(result, case),
        false,
        None,
        ledger,
        case,
        log_path,
        &execute_action,
        scribble,
    )
}

fn normalize(result: SessionResult, case: &ExceptionCase) -> Completion {
    match result {
        SessionResult::Result(raw) => {
            match result::parse(
                &raw,
                case.dropr_task_id.as_deref(),
                &case.worker_id,
                &worker_session_alive,
            ) {
                Ok(value) => Completion {
                    outcome: value.outcome,
                    action: value.action,
                    reason: value.reason,
                    action_warning: value.action_error,
                },
                Err(error) => escalation(format!("malformed result.json: {error}")),
            }
        }
        SessionResult::TimedOut => escalation("triage session timed out".into()),
        SessionResult::Missing => escalation("triage session exited without result.json".into()),
        SessionResult::AuthFailed(detail) => escalation(format!(
            "{}: {detail}",
            crate::overseer::session::auth::REASON
        )),
        SessionResult::LaunchFailed(error) => escalation(format!("triage session failed: {error}")),
    }
}

fn escalation(reason: String) -> Completion {
    Completion {
        outcome: Outcome::Escalate,
        action: None,
        reason,
        action_warning: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_completion(
    mut completion: Completion,
    replay: bool,
    marker: Option<&Path>,
    ledger: &mut Ledger,
    case: &ExceptionCase,
    log_path: &Path,
    action: &dyn Fn(&TriageAction, &ExceptionCase) -> Result<()>,
    scribble: &dyn Fn(&str, &str, &str) -> crate::dropr::WriteResult,
) -> Result<()> {
    if !replay
        && let Some(request) = &completion.action
        && let Err(error) = action(request, case)
    {
        completion = escalation(format!("triage action failed: {error}"));
        if let Some(marker) = marker {
            write_marker(marker, &completion)?;
        }
    }
    // A schema mismatch `result::parse` already recovered from: the action
    // was dropped, not the whole completion, so this is worth a line in
    // `decisions.jsonl` without spending the outcome on it — logged once,
    // on the pass that first read it, not on every later replay.
    if !replay && let Some(warning) = &completion.action_warning {
        log(
            log_path,
            DecisionKind::Hold,
            case,
            &format!("triage action ignored: {warning}"),
        )?;
    }
    match completion.outcome {
        Outcome::Skip => {
            if !ledger.skip_list.contains(&case.task_id) {
                ledger.skip_list.push(case.task_id.clone());
            }
            log(log_path, DecisionKind::Skip, case, &completion.reason)
        }
        Outcome::Escalate => {
            if let Some(entry) = ledger
                .entries
                .iter_mut()
                .find(|entry| entry.task_id == case.task_id)
            {
                entry.phase = LedgerPhase::Escalated;
                // Triage's own decision, not a worker's report — see
                // `LedgerEntry::worker_escalated`. Unrelated activity
                // elsewhere is no evidence that whatever triage escalated
                // over is resolved.
                entry.worker_escalated = false;
                // Triage is the second place an entry can reach a terminal
                // phase, and reconciliation only stamps the transitions it
                // makes itself — an entry escalated here is already terminal by
                // the time the next pass reads it, so this is its only chance
                // to record when it settled. Kept if it is already set, so a
                // repeat escalation cannot move the timestamp.
                entry.settled_at.get_or_insert_with(Utc::now);
                // A triage escalation never goes through `merge_hold::charge`,
                // so it never earns a reconsideration budget the way a
                // hold-cap escalation does. Without one, a finished, green
                // pull request whose triage output was a transient hiccup
                // (a malformed action, a timed-out session) sits parked
                // forever even after the condition clears — see dropr:401.
                // An entry with no pull request yet has nothing the merge
                // pass can read, so it stays parked instead.
                if entry.pr_url.is_some() {
                    entry.grant_merge_reconsideration(TRIAGE_ESCALATION);
                }
            }
            // The note is what an operator reading dropr sees; without it the
            // escalation is there with no explanation attached, so a lost
            // note is folded into this same escalation's reason rather than
            // logged as one of its own — a write robco failed at is robco's
            // problem, not a second operator decision, and the worker
            // failure this escalation is actually about must still notify
            // exactly once (dropr:556).
            //
            // `case.task_id` is not necessarily a dropr task — for an entry
            // adopted from a live agent it is the agent id — so the write
            // targets `case.dropr_task_id` instead, and is skipped entirely
            // when the case has no known dropr task: there is nothing to
            // record it against, and nothing failed either (dropr:531).
            let mut reason = completion.reason.clone();
            if !replay
                && let Some(task_id) = &case.dropr_task_id
                && let Err(error) = scribble(task_id, &case.repo, &completion.reason)
            {
                reason = format!("{reason} (escalation note not recorded in dropr: {error})");
            }
            log(log_path, DecisionKind::Escalate, case, &reason)
        }
        Outcome::Resolved => log(log_path, DecisionKind::Hold, case, &completion.reason),
    }
}

fn write_marker(path: &Path, completion: &Completion) -> Result<()> {
    fs::create_dir_all(path.parent().expect("outcome marker parent"))?;
    let temp = path.with_extension(format!("json.{}.tmp", nanoid!()));
    let written = serde_json::to_vec_pretty(completion)
        .map_err(Into::into)
        .and_then(|raw| fs::write(&temp, raw).map_err(Into::into))
        .and_then(|()| fs::rename(&temp, path).map_err(Into::into));
    if let Err(error) = written {
        let _ = fs::remove_file(temp);
        return Err(error);
    }
    Ok(())
}

fn dropr_scribble(task_id: &str, repo: &str, reason: &str) -> crate::dropr::WriteResult {
    let repo_url = crate::overseer::repo_lookup::repo_url_for(repo);
    crate::dropr::scribble_create_timeout(task_id, repo_url.as_deref(), reason, COMMAND_TIMEOUT)
}

fn log(path: &Path, kind: DecisionKind, case: &ExceptionCase, reason: &str) -> Result<()> {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(case.task_id.clone());
    entry.repo = Some(case.repo.clone());
    entry.source = Some("triage".into());
    logging::append_to(path, &entry)
}

#[cfg(test)]
pub(super) fn replay_test(
    result: SessionResult,
    ledger: &mut Ledger,
    case: &ExceptionCase,
    case_dir: &Path,
    log_path: &Path,
    action: &dyn Fn(&TriageAction, &ExceptionCase) -> Result<()>,
) -> Result<()> {
    complete_session_result_with(
        result,
        ledger,
        case,
        case_dir,
        log_path,
        action,
        &|_, _, _| Ok(()),
    )
}
