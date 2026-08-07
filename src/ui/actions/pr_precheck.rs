use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use crate::locale::t;

use super::super::{App, Mode};

pub(in crate::ui) struct PrPrecheckJob {
    receiver: Receiver<std::result::Result<(), String>>,
}

struct PrPrecheckTarget {
    repo_path: PathBuf,
    branch: String,
    tmux_session: String,
}

impl App {
    pub(in crate::ui) fn open_pr_dialog_with_precheck(
        &mut self,
        repo_path: PathBuf,
        agent_id: String,
        branch: String,
        tmux_session: String,
    ) {
        self.mode = Mode::PrPrecheck {
            repo_path: repo_path.clone(),
            agent_id,
            branch: branch.clone(),
        };
        self.pr_precheck_job = Some(PrPrecheckJob {
            receiver: spawn(PrPrecheckTarget {
                repo_path,
                branch,
                tmux_session,
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
    fn finish_pr_precheck(&mut self, result: std::result::Result<(), String>) {
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
        } = std::mem::replace(&mut self.mode, Mode::Normal)
        else {
            unreachable!("checked above")
        };
        match result {
            Ok(()) => {
                self.mode = Mode::ConfirmPr {
                    repo_path,
                    agent_id,
                    branch,
                    input: self.config.pr_prompt.clone().into(),
                };
            }
            Err(message) => self.show_message(message),
        }
    }
}

fn spawn(target: PrPrecheckTarget) -> Receiver<std::result::Result<(), String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(run_precheck(&target));
    });
    receiver
}

fn run_precheck(target: &PrPrecheckTarget) -> std::result::Result<(), String> {
    crate::pr::precheck(&target.repo_path, &target.branch, &target.tmux_session)
}

#[cfg(test)]
#[path = "pr_precheck_tests.rs"]
mod tests;
