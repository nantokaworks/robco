use chrono::Utc;

use crate::{
    git::{PrCheckView, PrChecks, PrState},
    locale::{fmt, t},
    overseer::runtime_request::{self, RuntimeRequest},
};

use super::{
    super::{App, LandPlan, Mode},
    pr_precheck::PrPrecheckRequest,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum LandDecision {
    Confirm(LandPlan, Option<String>),
    Cleanup,
    Closed,
    Failed(Vec<String>),
}

fn decide(state: PrState, view: Option<PrCheckView>) -> LandDecision {
    let (checks, head) = view
        .map(|view| (Some(view.checks), Some(view.head)))
        .unwrap_or_default();
    match (state, checks) {
        (PrState::Absent, _) => LandDecision::Confirm(LandPlan::OpenPrThenQueue, None),
        (PrState::Merged, _) => LandDecision::Cleanup,
        (PrState::ClosedUnmerged, _) => LandDecision::Closed,
        (PrState::Open, Some(PrChecks::Green)) => LandDecision::Confirm(LandPlan::MergeNow, head),
        (PrState::Open, Some(PrChecks::Waiting)) => {
            LandDecision::Confirm(LandPlan::QueueApproval, head)
        }
        (PrState::Open, Some(PrChecks::Failed(names))) => LandDecision::Failed(names),
        (PrState::Open, None) => LandDecision::Confirm(LandPlan::QueueApproval, head),
    }
}

impl App {
    pub(in crate::ui) fn offer_land(
        &mut self,
        state: PrState,
        checks: Option<PrCheckView>,
        repo: usize,
        agent: usize,
        branch: &str,
    ) {
        match decide(state, checks) {
            LandDecision::Confirm(plan, head) => {
                self.mode = Mode::ConfirmMerge {
                    repo,
                    agent,
                    plan,
                    head,
                }
            }
            LandDecision::Cleanup => self.mode = Mode::ConfirmCleanup { repo, agent },
            LandDecision::Closed => self.show_message(fmt(
                self.locale,
                "PR for {} was closed without merging; reopen it or open a new one",
                &[branch],
            )),
            LandDecision::Failed(names) => {
                let names = if names.is_empty() {
                    t(self.locale, "an unnamed check").to_string()
                } else {
                    names.join(", ")
                };
                self.show_message(fmt(
                    self.locale,
                    "Nothing was merged because these checks failed: {}",
                    &[&names],
                ));
            }
        }
    }

    pub(in crate::ui) fn confirm_land(
        &mut self,
        repo: usize,
        agent: usize,
        plan: LandPlan,
        head: Option<String>,
    ) {
        match plan {
            LandPlan::MergeNow => self.start_merge(repo, agent),
            LandPlan::QueueApproval => {
                let target = self.registry.repos[repo].agents[agent].id.clone();
                self.mode = Mode::Normal;
                let Some(head) = head else {
                    self.show_message("pull request head is empty");
                    return;
                };
                self.queue_merge_approval(&target, head, false);
            }
            LandPlan::OpenPrThenQueue => {
                let repo_node = &self.registry.repos[repo];
                let selected = &repo_node.agents[agent];
                let target = selected.id.clone();
                let approval_head =
                    match crate::git::local_branch_commit(&repo_node.path, &selected.branch) {
                        Ok(head) => head,
                        Err(error) => {
                            self.mode = Mode::Normal;
                            self.show_message(error.to_string());
                            return;
                        }
                    };
                let display_id = self.task_display_id(selected);
                self.open_pr_dialog_with_precheck(PrPrecheckRequest {
                    repo_path: repo_node.path.clone(),
                    agent_id: target,
                    branch: selected.branch.clone(),
                    tmux_session: selected.tmux_session.clone(),
                    worktree_path: selected.worktree_path.clone(),
                    title: selected.title.clone(),
                    display_id,
                    approval_head: Some(approval_head),
                });
            }
        }
    }

    pub(in crate::ui) fn queue_merge_approval(
        &mut self,
        target: &str,
        head: String,
        after_pr_prompt: bool,
    ) {
        let request = RuntimeRequest::MergeApproval {
            source: "tui".into(),
            target: target.into(),
            head,
            at: Utc::now(),
        };
        match runtime_request::enqueue(request) {
            // The toast lasts four seconds; the daemon may take a whole poll
            // interval. Record the approval so the agent's own row keeps
            // saying robco has it, long after the message is gone
            // (dropr:545). Only on success — a request that never reached
            // the queue must not claim robco is acting on it.
            Ok(()) => {
                self.note_merge_approval_queued(target);
                let message = if after_pr_prompt {
                    "PR requested and approval queued; it will merge once the checks pass"
                } else {
                    "Approval queued; it will merge once the checks pass"
                };
                self.show_message(t(self.locale, message));
            }
            Err(err) => self.show_message(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, registry::Registry};

    /// dropr:523: `m` must never answer "the Overseer cannot land this" —
    /// `queue_merge_approval` no longer checks the ledger at all before
    /// enqueueing. A worker the ledger has never adopted (dropr:874's shape:
    /// the daemon has not run adoption for it yet) still gets its approval
    /// queued; the daemon side (`ledger::ensure_landable`) is what adopts or
    /// revives the entry when the request drains.
    #[test]
    fn queue_merge_approval_never_refuses_regardless_of_the_ledger() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        assert!(app.overseer_snapshot.ledger.entries.is_empty());

        app.queue_merge_approval("legacy-worker", "deadbeef".into(), false);

        let message = app.message.as_ref().map(|(message, _)| message.clone());
        assert_eq!(
            message.as_deref(),
            Some("Approval queued; it will merge once the checks pass")
        );
    }

    /// dropr:545: the toast is gone in four seconds, so the row has to carry
    /// the same fact. A queued approval is recorded the moment the request
    /// reaches the queue.
    #[test]
    fn a_queued_approval_is_recorded_for_the_agents_own_row() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());

        app.queue_merge_approval("legacy-worker", "deadbeef".into(), false);

        assert!(app.merge_approval_queued("legacy-worker"));
        assert!(!app.merge_approval_queued("some-other-worker"));
    }

    #[test]
    fn every_pull_request_state_maps_to_the_promised_land_action() {
        assert_eq!(
            decide(PrState::Absent, None),
            LandDecision::Confirm(LandPlan::OpenPrThenQueue, None)
        );
        assert_eq!(
            decide(
                PrState::Open,
                Some(PrCheckView {
                    checks: PrChecks::Waiting,
                    head: "abc".into(),
                }),
            ),
            LandDecision::Confirm(LandPlan::QueueApproval, Some("abc".into()))
        );
        assert_eq!(
            decide(
                PrState::Open,
                Some(PrCheckView {
                    checks: PrChecks::Green,
                    head: "abc".into(),
                }),
            ),
            LandDecision::Confirm(LandPlan::MergeNow, Some("abc".into()))
        );
        assert_eq!(
            decide(
                PrState::Open,
                Some(PrCheckView {
                    checks: PrChecks::Failed(vec!["build".into()]),
                    head: "abc".into(),
                }),
            ),
            LandDecision::Failed(vec!["build".into()])
        );
        assert_eq!(decide(PrState::Merged, None), LandDecision::Cleanup);
        assert_eq!(decide(PrState::ClosedUnmerged, None), LandDecision::Closed);
    }
}
