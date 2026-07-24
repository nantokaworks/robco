//! The claim the overseer takes on a candidate immediately before it spawns.
//!
//! Everything upstream of this point — the ready list, the deterministic gate,
//! the judge round — reasons about a snapshot that is minutes old by the time a
//! worker actually starts. This is the last read, and it is followed
//! immediately by the write that makes the task ours, so an agent that claimed
//! the task in the meantime is seen rather than raced.

use chrono::{DateTime, Utc};

use super::Candidate;
use crate::dropr::{ClaimAttempt, TaskClaim};
use crate::overseer::{exec::COMMAND_TIMEOUT, logging::DecisionKind};

/// What the pre-spawn claim check decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClaimGate {
    /// Dispatch may proceed. `expired_holder` names the agent whose lapsed
    /// claim we are taking over, so the takeover is logged rather than silent.
    Proceed { expired_holder: Option<String> },
    /// Stand off, with the reason recorded against the candidate.
    Hold(String),
}

/// Whether a re-read task is still ours to take.
///
/// `state` is `None` when dropr could not be reached. That is deliberately a
/// hold: dispatching on an unknown claim is the exact failure this gate exists
/// to prevent, and a held candidate costs one pass, not a duplicated worker.
pub(super) fn evaluate(state: Option<&TaskClaim>, agent: &str, now: DateTime<Utc>) -> ClaimGate {
    let Some(state) = state else {
        return ClaimGate::Hold("claim_unreadable".into());
    };
    if let Some(holder) = state.holder(agent) {
        if state.held_by_other(agent, now) {
            return ClaimGate::Hold(format!("claimed_elsewhere:{holder}"));
        }
        // The claim lapsed: a manual run that outlived its TTL, or an agent
        // that died holding it.
        return ClaimGate::Proceed {
            expired_holder: Some(holder.to_owned()),
        };
    }
    if state.status == "open" || state.claimed_by_agent_id.as_deref() == Some(agent) {
        return ClaimGate::Proceed {
            expired_holder: None,
        };
    }
    // Closed, draft, or in progress with nobody holding it: whatever moved it
    // out of `open` did so outside the overseer's view.
    ClaimGate::Hold(format!("task_not_open:{}", state.status))
}

/// Result of acquiring the claim for a candidate.
pub(super) enum Acquired {
    Claimed,
    Held(String),
}

/// Re-reads the task, decides, and — when it is still free — takes the claim
/// for `agent` before any worker exists to compete for it.
pub(super) fn acquire(
    candidate: &Candidate,
    agent: &str,
    now: DateTime<Utc>,
) -> crate::Result<Acquired> {
    if candidate.workspace.is_empty() {
        // Without a workspace there is no task to re-read; the candidate came
        // from a repository dropr does not know about.
        return Ok(Acquired::Held("claim_workspace_unknown".into()));
    }
    let state = crate::dropr::task_claim(&candidate.workspace, &candidate.task_id, COMMAND_TIMEOUT);
    let expired_holder = match evaluate(state.as_ref(), agent, now) {
        ClaimGate::Hold(reason) => return Ok(Acquired::Held(reason)),
        ClaimGate::Proceed { expired_holder } => expired_holder,
    };
    match crate::dropr::claim_task(
        &candidate.workspace,
        &candidate.task_id,
        agent,
        COMMAND_TIMEOUT,
    ) {
        ClaimAttempt::Claimed => {
            if let Some(holder) = expired_holder {
                // Logged once the takeover is real rather than merely intended.
                super::runtime::log_candidate(
                    DecisionKind::Dispatch,
                    candidate,
                    &format!("claim_expired:{holder}"),
                )?;
            }
            Ok(Acquired::Claimed)
        }
        // dropr refuses a task somebody else locked between the read above and
        // this call. That is the same stand-off the read catches, so it is
        // reported the same way — by name where the name is still there to be
        // read.
        ClaimAttempt::Refused(reason) => Ok(Acquired::Held(refusal_hold(candidate, agent, reason))),
        ClaimAttempt::Unavailable => Ok(Acquired::Held("claim_unavailable".into())),
    }
}

fn refusal_hold(candidate: &Candidate, agent: &str, reason: String) -> String {
    crate::dropr::task_claim(&candidate.workspace, &candidate.task_id, COMMAND_TIMEOUT)
        .as_ref()
        .and_then(|state| state.holder(agent))
        .map_or_else(
            || format!("claim_refused:{reason}"),
            |holder| format!("claimed_elsewhere:{holder}"),
        )
}

/// Hands the claim back when the worker it was taken for never started.
pub(super) fn release(candidate: &Candidate, agent: &str) -> bool {
    !candidate.workspace.is_empty()
        && crate::dropr::release_claim(
            &candidate.workspace,
            &candidate.task_id,
            agent,
            COMMAND_TIMEOUT,
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(minutes * 60, 0).unwrap()
    }

    fn state(status: &str, holder: Option<&str>, expires: Option<i64>) -> TaskClaim {
        TaskClaim {
            status: status.into(),
            claimed_by_agent_id: holder.map(str::to_owned),
            claim_expires_at: expires.map(at),
        }
    }

    #[test]
    fn an_open_task_is_dispatchable() {
        assert_eq!(
            evaluate(Some(&state("open", None, None)), "overseer", at(0)),
            ClaimGate::Proceed {
                expired_holder: None
            }
        );
    }

    #[test]
    fn a_task_claimed_since_the_candidate_was_gathered_stands_off() {
        // The race this gate exists for: the ready list said `open`, an operator
        // claimed it during the judge round, and the spawn is now a duplicate.
        assert_eq!(
            evaluate(
                Some(&state("in_progress", Some("manual-run"), Some(30))),
                "overseer",
                at(10),
            ),
            ClaimGate::Hold("claimed_elsewhere:manual-run".into())
        );
    }

    #[test]
    fn an_expired_claim_is_taken_over_by_name() {
        assert_eq!(
            evaluate(
                Some(&state("in_progress", Some("manual-run"), Some(30))),
                "overseer",
                at(31),
            ),
            ClaimGate::Proceed {
                expired_holder: Some("manual-run".into())
            }
        );
    }

    #[test]
    fn our_own_claim_does_not_block_the_spawn() {
        // A re-read after we already claimed must not read as somebody else's.
        assert_eq!(
            evaluate(
                Some(&state("in_progress", Some("overseer"), Some(30))),
                "overseer",
                at(10),
            ),
            ClaimGate::Proceed {
                expired_holder: None
            }
        );
    }

    #[test]
    fn a_task_that_left_open_without_a_claim_stands_off() {
        assert_eq!(
            evaluate(Some(&state("closed", None, None)), "overseer", at(0)),
            ClaimGate::Hold("task_not_open:closed".into())
        );
    }

    #[test]
    fn an_unreadable_claim_stands_off() {
        // Fail closed: an unknown claim is not permission to spawn.
        assert_eq!(
            evaluate(None, "overseer", at(0)),
            ClaimGate::Hold("claim_unreadable".into())
        );
    }
}
