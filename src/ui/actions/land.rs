use chrono::Utc;

use crate::{
    git::{PrCheckView, PrChecks, PrState},
    locale::{fmt, t},
    overseer::runtime_request::{self, RuntimeRequest},
};

use super::super::{App, LandPlan, Mode};

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
                if !self.overseer_can_land(&target) {
                    self.show_not_overseer_managed();
                    return;
                }
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
                if !self.overseer_can_land(&target) {
                    self.mode = Mode::Normal;
                    self.show_not_overseer_managed();
                    return;
                }
                let approval_head =
                    match crate::git::local_branch_commit(&repo_node.path, &selected.branch) {
                        Ok(head) => head,
                        Err(error) => {
                            self.mode = Mode::Normal;
                            self.show_message(error.to_string());
                            return;
                        }
                    };
                self.open_pr_dialog_with_precheck(
                    repo_node.path.clone(),
                    target,
                    selected.branch.clone(),
                    selected.tmux_session.clone(),
                    Some(approval_head),
                );
            }
        }
    }

    fn overseer_can_land(&self, target: &str) -> bool {
        self.overseer_snapshot.ledger.entries.iter().any(|entry| {
            entry.agent_id == target && !crate::overseer::ledger::terminal(entry.phase)
        })
    }

    fn show_not_overseer_managed(&mut self) {
        self.show_message(t(
            self.locale,
            "This agent is not one the Overseer daemon can land; merge it directly instead",
        ));
    }

    pub(in crate::ui) fn queue_merge_approval(
        &mut self,
        target: &str,
        head: String,
        after_pr_prompt: bool,
    ) {
        if !self.overseer_can_land(target) {
            self.show_not_overseer_managed();
            return;
        }
        let request = RuntimeRequest::MergeApproval {
            source: "tui".into(),
            target: target.into(),
            head,
            at: Utc::now(),
        };
        match runtime_request::enqueue(request) {
            Ok(()) if after_pr_prompt => self.show_message(t(
                self.locale,
                "PR requested and approval queued; it will merge once the checks pass",
            )),
            Ok(()) => self.show_message(t(
                self.locale,
                "Approval queued; it will merge once the checks pass",
            )),
            Err(err) => self.show_message(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
