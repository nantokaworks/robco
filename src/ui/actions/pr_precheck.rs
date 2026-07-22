use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use super::super::{App, Mode};

pub(in crate::ui) struct PrPrecheckJob {
    pub repo_path: PathBuf,
    pub agent_id: String,
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
        self.mode = Mode::ConfirmPr {
            repo_path: repo_path.clone(),
            agent_id: agent_id.clone(),
            branch: branch.clone(),
            input: self.config.pr_prompt.clone(),
        };
        self.pr_precheck_job = Some(PrPrecheckJob {
            repo_path: repo_path.clone(),
            agent_id,
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

    pub(in crate::ui) fn pr_precheck_active_for(
        &self,
        repo_path: &std::path::Path,
        agent_id: &str,
    ) -> bool {
        self.pr_precheck_job()
            .is_some_and(|job| job.repo_path == repo_path && job.agent_id == agent_id)
    }

    pub(in crate::ui) fn drain_pr_precheck_events(&mut self) {
        let result = self.pr_precheck_job().map(|job| job.receiver.try_recv());
        match result {
            Some(Ok(result)) => self.finish_pr_precheck(result),
            Some(Err(TryRecvError::Empty)) | None => {}
            Some(Err(TryRecvError::Disconnected)) if self.pr_precheck_job.is_some() => self
                .finish_pr_precheck(Err(
                    "PR pre-check worker terminated unexpectedly".to_string()
                )),
            Some(Err(TryRecvError::Disconnected)) => {}
        }
    }

    fn finish_pr_precheck(&mut self, result: std::result::Result<(), String>) {
        let Some(_job) = self.pr_precheck_job.take() else {
            return;
        };
        match result {
            Ok(()) => {}
            Err(message) => {
                if matches!(self.mode, Mode::ConfirmPr { .. }) {
                    self.mode = Mode::Normal;
                }
                self.show_message(message);
            }
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
    let running =
        crate::tmux::has_session(&target.tmux_session).map_err(|error| error.to_string())?;
    if !running {
        return Err("agent session is not running".to_string());
    }
    let exists = crate::git::pr_exists(&target.repo_path, &target.branch)
        .map_err(|error| error.to_string())?;
    if exists {
        return Err(format!("PR already open for {}", target.branch));
    }
    Ok(())
}

#[cfg(test)]
#[path = "pr_precheck_tests.rs"]
mod tests;
