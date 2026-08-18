use chrono::Utc;

use crate::overseer::{
    daemon::pull_request,
    ledger::{Ledger, LedgerEntry, MergeApproval},
    logging::{self, DecisionEntry, DecisionKind},
};

/// Reason seeded on an operator approval's reconsideration budget — see
/// `LedgerEntry::grant_merge_reconsideration`. Never a gate reason itself, so
/// the merge pass's first re-read of the pull request always counts as a
/// change against it.
const OPERATOR_APPROVAL: &str = "operator_approval";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LedgerRequest {
    Skip {
        task: String,
        user_id: String,
    },
    Retry {
        task: String,
        user_id: String,
    },
    /// An operator approval Discord's `!merge` queued against a pull
    /// request's ledger entry, whatever phase it is in — see
    /// `overseer::discord::merge_actions::merge`.
    Approve {
        task: String,
        user_id: String,
    },
}

impl LedgerRequest {
    pub(crate) fn attribution(&self) -> (&str, &str) {
        match self {
            Self::Skip { task, user_id }
            | Self::Retry { task, user_id }
            | Self::Approve { task, user_id } => (task, user_id),
        }
    }
}

pub(crate) fn apply(ledger: &mut Ledger, request: LedgerRequest) -> Result<(), String> {
    match request {
        LedgerRequest::Skip { task, .. } => {
            ledger.skip_list.retain(|id| id != &task);
            ledger.skip_list.push(task);
            Ok(())
        }
        LedgerRequest::Retry { task, .. } => {
            let aliases: Vec<_> = ledger
                .entries
                .iter()
                .filter(|entry| entry.task_id == task || entry.display_id == task)
                .flat_map(|entry| [&entry.task_id, &entry.display_id])
                .cloned()
                .collect();
            let mut found = false;
            for entry in ledger
                .entries
                .iter_mut()
                .filter(|entry| entry.task_id == task || entry.display_id == task)
            {
                entry.retries = 0;
                found = true;
            }
            if !found {
                return Err(format!("task not found: {task}"));
            }
            ledger
                .skip_list
                .retain(|id| id != &task && !aliases.contains(id));
            Ok(())
        }
        LedgerRequest::Approve { task, user_id } => {
            let Some(entry) = ledger
                .entries
                .iter_mut()
                .find(|entry| entry.task_id == task || entry.display_id == task)
            else {
                return Err(format!("task not found: {task}"));
            };
            let Some(url) = entry.pr_url.clone() else {
                return Err(format!("{task}: no pull request recorded"));
            };
            let repo = entry.repo.clone();
            // Re-read the pull request's current head rather than trusting one
            // carried on the request, the same reason
            // `runtime_request::grant_operator_override` does: the approval
            // names the revision the merge pass is about to see, not one taken
            // when the operator typed `!merge`.
            let value =
                pull_request::read(&repo, &url).map_err(|error| format!("{task}: {error}"))?;
            let head = pull_request::head_sha(&value).to_owned();
            record_approval(entry, repo, url, user_id, head)
                .map_err(|error| format!("{task}: {error}"))
        }
    }
}

/// Grants the approval and, because it may be the only thing that ever
/// re-arms an exhausted hold-cap budget, resets `merge_hold_recheck` too —
/// split out from [`apply`]'s `Approve` arm so it is testable without the
/// `gh pr view` call `pull_request::read` makes.
///
/// An entry the hold-cap already escalated may have spent its whole
/// `merge_hold_recheck` budget before this approval arrived — see
/// `daemon::merge_repo_pass::run`'s `reconsidering` check, which never
/// re-reads the gate once that budget is gone. An operator approval is a new
/// fact the old budget was never sized for, so it grants a fresh look the
/// same way a killed session or a triage escalation does
/// (`LedgerEntry::grant_merge_reconsideration`), rather than leaving the
/// entry unable to ever reach `merge_judge_gate::take_merge_approval`.
fn record_approval(
    entry: &mut LedgerEntry,
    repo: String,
    url: String,
    user_id: String,
    head: String,
) -> Result<(), String> {
    entry.merge_approval = Some(MergeApproval {
        head,
        granted_at: Utc::now(),
    });
    entry.grant_merge_reconsideration(OPERATOR_APPROVAL);
    let mut decision = DecisionEntry::new(
        DecisionKind::Merge,
        format!("{OPERATOR_APPROVAL}:recheck_budget_reset"),
    );
    decision.task = Some(entry.display_id.clone());
    decision.repo = Some(repo);
    decision.pr_url = Some(url);
    decision.user_id = Some(user_id);
    decision.source = Some("discord".into());
    logging::append(&decision).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::ledger::LedgerPhase;

    fn entry(task_id: &str, display_id: &str, pr_url: Option<&str>) -> LedgerEntry {
        LedgerEntry {
            task_id: task_id.into(),
            display_id: display_id.into(),
            repo: "/no/such/repo".into(),
            agent_id: "agent-1".into(),
            branch: "branch".into(),
            phase: LedgerPhase::PrOpened,
            dispatched_at: Utc::now(),
            settled_at: None,
            retries: 0,
            pr_url: pr_url.map(str::to_owned),
            branch_updates: 0,
            merge_judge_primes: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_judge_fail_safes: 0,
            merge_hold_cap_escalated: false,
            merge_hold_rechecks: 0,
            merge_hold_recheck_reason: None,
            merge_hold_recheck_head: None,
            prerequisite_wait: None,
            merge_hold_stuck_notified: false,
            escalation_notified_reason: None,
            escalation_notified_head: None,
            worker_escalated: false,
            operator_override: None,
            merge_approval: None,
        }
    }

    #[test]
    fn requests_only_mutate_daemon_owned_ledger() {
        let mut ledger = Ledger::default();
        apply(
            &mut ledger,
            LedgerRequest::Skip {
                task: "task-1".into(),
                user_id: "user-1".into(),
            },
        )
        .unwrap();
        assert_eq!(ledger.skip_list, ["task-1"]);
        assert!(
            apply(
                &mut ledger,
                LedgerRequest::Retry {
                    task: "missing".into(),
                    user_id: "user-1".into(),
                }
            )
            .is_err()
        );
    }

    #[test]
    fn approve_refuses_a_task_the_ledger_does_not_know() {
        let mut ledger = Ledger::default();
        assert!(
            apply(
                &mut ledger,
                LedgerRequest::Approve {
                    task: "missing".into(),
                    user_id: "user-1".into(),
                }
            )
            .is_err()
        );
    }

    #[test]
    fn approve_refuses_an_entry_with_no_pull_request_recorded() {
        let mut ledger = Ledger {
            entries: vec![entry("task-1", "#1", None)],
            ..Default::default()
        };
        assert!(
            apply(
                &mut ledger,
                LedgerRequest::Approve {
                    task: "#1".into(),
                    user_id: "user-1".into(),
                }
            )
            .is_err()
        );
        assert!(ledger.entries[0].merge_approval.is_none());
    }

    #[test]
    fn approve_resets_an_exhausted_recheck_budget() {
        let mut ledger_entry = entry(
            "task-1",
            "#1",
            Some("https://github.com/acme/widgets/pull/1"),
        );
        ledger_entry.phase = LedgerPhase::Escalated;
        ledger_entry.merge_hold_cap_escalated = true;
        // A budget already spent — exactly the entry `merge_repo_pass::run`'s
        // `reconsidering` check would otherwise never look at again.
        ledger_entry.merge_hold_rechecks = 10;
        record_approval(
            &mut ledger_entry,
            "/repo".into(),
            "https://github.com/acme/widgets/pull/1".into(),
            "user-1".into(),
            "deadbeef".into(),
        )
        .unwrap();
        assert_eq!(
            ledger_entry.merge_approval.as_ref().unwrap().head,
            "deadbeef"
        );
        assert_eq!(ledger_entry.merge_hold_rechecks, 0);
        assert!(ledger_entry.merge_hold_cap_escalated);
    }

    #[test]
    fn approve_matches_by_display_id_but_leaves_no_approval_when_the_pull_request_cannot_be_read() {
        let mut ledger = Ledger {
            entries: vec![entry(
                "task-1",
                "#1",
                Some("https://github.com/acme/widgets/pull/1"),
            )],
            ..Default::default()
        };
        assert!(
            apply(
                &mut ledger,
                LedgerRequest::Approve {
                    task: "#1".into(),
                    user_id: "user-1".into(),
                }
            )
            .is_err()
        );
        assert!(ledger.entries[0].merge_approval.is_none());
    }
}
