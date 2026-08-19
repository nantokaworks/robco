use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use crate::locale::{fmt, t};

use super::super::{App, Mode};

pub(in crate::ui) struct PrPrecheckJob {
    receiver: Receiver<std::result::Result<PrPrecheckOutcome, String>>,
}

/// Everything the caller already knows about the target and cannot be
/// re-derived on the background thread: the agent's ledger-or-branch task
/// number and its worktree path, needed only by the no-session fallback (see
/// [`super::pr_fallback`]).
pub(in crate::ui) struct PrPrecheckRequest {
    pub repo_path: PathBuf,
    pub agent_id: String,
    pub branch: String,
    pub tmux_session: String,
    pub worktree_path: PathBuf,
    pub title: String,
    pub display_id: Option<String>,
    pub approval_head: Option<String>,
}

struct PrPrecheckTarget {
    repo_path: PathBuf,
    branch: String,
    tmux_session: String,
    worktree_path: PathBuf,
    title: String,
    display_id: Option<String>,
}

/// What the background thread found. A live session still writes its own
/// pull request body through the `ConfirmPr` dialog; a dead one gets a pull
/// request robco already opened itself, so there is nothing left to confirm.
enum PrPrecheckOutcome {
    OpenDialog,
    Created(String),
}

impl App {
    pub(in crate::ui) fn open_pr_dialog_with_precheck(&mut self, request: PrPrecheckRequest) {
        let PrPrecheckRequest {
            repo_path,
            agent_id,
            branch,
            tmux_session,
            worktree_path,
            title,
            display_id,
            approval_head,
        } = request;
        self.mode = Mode::PrPrecheck {
            repo_path: repo_path.clone(),
            agent_id,
            branch: branch.clone(),
            approval_head,
        };
        self.pr_precheck_job = Some(PrPrecheckJob {
            receiver: spawn(PrPrecheckTarget {
                repo_path,
                branch,
                tmux_session,
                worktree_path,
                title,
                display_id,
            }),
        });
    }

    pub(in crate::ui) fn pr_precheck_job(&self) -> Option<&PrPrecheckJob> {
        self.pr_precheck_job.as_ref()
    }

    pub(in crate::ui) fn drain_pr_precheck_events(&mut self) {
        let result = self.pr_precheck_job().map(|job| job.receiver.try_recv());
        match result {
            Some(Ok(result)) => self.finish_pr_precheck(result),
            Some(Err(TryRecvError::Empty)) | None => {}
            Some(Err(TryRecvError::Disconnected)) if self.pr_precheck_job.is_some() => self
                .finish_pr_precheck(Err(t(
                    self.locale,
                    "PR pre-check worker terminated unexpectedly",
                )
                .to_string())),
            Some(Err(TryRecvError::Disconnected)) => {}
        }
    }

    /// Applies a finished precheck's result to the progress modal, if it is
    /// still open. A precheck racing a cancel (or a second P press) lands
    /// here with `self.mode` already back to `Normal`, and is dropped.
    fn finish_pr_precheck(&mut self, result: std::result::Result<PrPrecheckOutcome, String>) {
        if self.pr_precheck_job.take().is_none() {
            return;
        }
        if !matches!(self.mode, Mode::PrPrecheck { .. }) {
            return;
        }
        let Mode::PrPrecheck {
            repo_path,
            agent_id,
            branch,
            approval_head,
        } = std::mem::replace(&mut self.mode, Mode::Normal)
        else {
            unreachable!("checked above")
        };
        match result {
            Ok(PrPrecheckOutcome::OpenDialog) => {
                self.mode = Mode::ConfirmPr {
                    repo_path,
                    agent_id,
                    branch,
                    input: self.config.pr_prompt.clone().into(),
                    approval_head,
                };
            }
            Ok(PrPrecheckOutcome::Created(pr_url)) => match approval_head {
                Some(head) => self.queue_merge_approval(&agent_id, head, true),
                None => self.show_message(fmt(self.locale, "PR opened: {}", &[&pr_url])),
            },
            Err(message) => self.show_message(message),
        }
    }
}

fn spawn(target: PrPrecheckTarget) -> Receiver<std::result::Result<PrPrecheckOutcome, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(run_precheck(&target));
    });
    receiver
}

/// A branch's pull request status decides where the request goes next: a
/// pull request already open is refused outright, otherwise a live session
/// authors its own body while a dead one gets robco's fallback creation.
#[derive(Debug, PartialEq, Eq)]
enum PrPrecheckRoute {
    OpenDialog,
    CreateFallback,
    PrOpen,
}

fn route(running: bool, pr_open: bool) -> PrPrecheckRoute {
    if pr_open {
        return PrPrecheckRoute::PrOpen;
    }
    if running {
        PrPrecheckRoute::OpenDialog
    } else {
        PrPrecheckRoute::CreateFallback
    }
}

fn run_precheck(target: &PrPrecheckTarget) -> std::result::Result<PrPrecheckOutcome, String> {
    let running =
        crate::tmux::has_session(&target.tmux_session).map_err(|error| error.to_string())?;
    let pr_open = crate::git::pr_exists(&target.repo_path, &target.branch)
        .map_err(|error| error.to_string())?;
    match route(running, pr_open) {
        PrPrecheckRoute::PrOpen => Err(format!("PR already open for {}", target.branch)),
        PrPrecheckRoute::OpenDialog => Ok(PrPrecheckOutcome::OpenDialog),
        PrPrecheckRoute::CreateFallback => {
            let display_id = target.display_id.clone().ok_or_else(|| {
                "no dropr task found for this branch; cannot open a pull request without a Close Dropr line"
                    .to_string()
            })?;
            super::pr_fallback::create_fallback_pr(
                &target.repo_path,
                &target.branch,
                &target.worktree_path,
                &target.title,
                &display_id,
            )
            .map(PrPrecheckOutcome::Created)
        }
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;

    #[test]
    fn a_live_session_opens_the_dialog_and_a_dead_one_falls_back() {
        assert_eq!(route(true, false), PrPrecheckRoute::OpenDialog);
        assert_eq!(route(false, false), PrPrecheckRoute::CreateFallback);
    }

    #[test]
    fn an_existing_open_pull_request_is_refused_either_way() {
        assert_eq!(route(true, true), PrPrecheckRoute::PrOpen);
        assert_eq!(route(false, true), PrPrecheckRoute::PrOpen);
    }
}

#[cfg(test)]
#[path = "pr_precheck_tests.rs"]
mod tests;
