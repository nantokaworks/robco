use chrono::Utc;

use crate::overseer::{
    daemon::pull_request,
    ledger::{Ledger, MergeApproval},
};

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
    /// An operator approval Discord's `!merge` queued against a pull request
    /// that had not yet reached `Escalated` — see
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
        LedgerRequest::Approve { task, .. } => {
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
            entry.merge_approval = Some(MergeApproval {
                head: pull_request::head_sha(&value).to_owned(),
                granted_at: Utc::now(),
            });
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::ledger::{LedgerEntry, LedgerPhase};

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
