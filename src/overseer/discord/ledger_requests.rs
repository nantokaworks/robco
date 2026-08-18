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
    /// A `!run <task>` naming a dropr task to dispatch. Unlike the other
    /// three variants, `apply` never handles this one — dispatching needs
    /// `Config`, `now`, and the post-reconcile ledger, none of which `apply`
    /// has, so the daemon loop pulls `Run` requests out of the queue
    /// separately (see `daemon::discord_sync::apply_ledger_requests`'s
    /// `PendingRun` return value) and feeds them to
    /// `dispatch::run_named` alongside its own `dispatch_pass` call. Still
    /// travels over this same channel, so the Discord thread never touches
    /// the ledger — only the queuing mechanism this variant needs.
    Run {
        task: String,
        user_id: String,
    },
}

impl LedgerRequest {
    pub(crate) fn attribution(&self) -> (&str, &str) {
        match self {
            Self::Skip { task, user_id }
            | Self::Retry { task, user_id }
            | Self::Approve { task, user_id }
            | Self::Run { task, user_id } => (task, user_id),
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
        // Never reached through the normal drain path: `apply_ledger_requests`
        // pulls `Run` out of the queue before calling `apply` — see the
        // variant's doc comment. Kept as a harmless no-op rather than a
        // wildcard arm, so a fifth variant added later cannot silently fall
        // through here unnoticed.
        LedgerRequest::Run { .. } => Ok(()),
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
#[path = "ledger_requests_tests.rs"]
mod tests;
