use super::{
    ExceptionCase,
    actions::{execute_action, worker_session_alive},
    result::{self, Outcome, ParseError, TriageAction},
};
use crate::{
    Result,
    chief::{
        exec::COMMAND_TIMEOUT,
        ledger::{Ledger, LedgerPhase},
        logging::{self, DecisionEntry, DecisionKind},
        session::SessionResult,
    },
};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Completion {
    outcome: Outcome,
    action: Option<TriageAction>,
    reason: String,
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
    scribble: &dyn Fn(&str, &str) -> Result<()>,
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
    scribble: &dyn Fn(&str, &str) -> Result<()>,
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
            match result::parse(&raw, &case.task_id, &case.worker_id, &worker_session_alive) {
                Ok(value) => Completion {
                    outcome: value.outcome,
                    action: value.action,
                    reason: value.reason,
                },
                Err(ParseError::Malformed(error)) => {
                    escalation(format!("malformed result.json: {error}"))
                }
                Err(ParseError::RejectedAction(error)) => {
                    escalation(format!("rejected triage action: {error}"))
                }
            }
        }
        SessionResult::TimedOut => escalation("triage session timed out".into()),
        SessionResult::Missing => escalation("triage session exited without result.json".into()),
        SessionResult::LaunchFailed(error) => escalation(format!("triage session failed: {error}")),
    }
}

fn escalation(reason: String) -> Completion {
    Completion {
        outcome: Outcome::Escalate,
        action: None,
        reason,
    }
}

fn apply_completion(
    mut completion: Completion,
    replay: bool,
    marker: Option<&Path>,
    ledger: &mut Ledger,
    case: &ExceptionCase,
    log_path: &Path,
    action: &dyn Fn(&TriageAction, &ExceptionCase) -> Result<()>,
    scribble: &dyn Fn(&str, &str) -> Result<()>,
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
            }
            if !replay && let Err(error) = scribble(&case.task_id, &completion.reason) {
                log(
                    log_path,
                    DecisionKind::Hold,
                    case,
                    &format!("escalation scribble failed: {error}"),
                )?;
            }
            log(log_path, DecisionKind::Escalate, case, &completion.reason)
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

fn dropr_scribble(task_id: &str, reason: &str) -> Result<()> {
    crate::dropr::scribble_create_timeout(task_id, reason, COMMAND_TIMEOUT)
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
    complete_session_result_with(result, ledger, case, case_dir, log_path, action, &|_, _| {
        Ok(())
    })
}
