use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use crate::locale::{fmt, t};

use super::super::App;

pub(in crate::ui) struct CloneJob {
    pub url: String,
    receiver: Receiver<std::result::Result<PathBuf, String>>,
}

impl App {
    pub(in crate::ui) fn add_repo_input(&mut self, input: &str) {
        let mut parts = input.split_whitespace();
        let first = parts.next().unwrap_or_default();
        if crate::clone::looks_like_git_url(first) {
            let branch = parts.next().map(str::to_string);
            if parts.next().is_some() {
                self.show_message(t(self.locale, "format: <git-url> [branch]"));
                return;
            }
            self.start_clone(first.to_string(), branch);
        } else {
            self.add_repo_path(input.trim());
        }
    }

    fn start_clone(&mut self, url: String, branch: Option<String>) {
        if let Some(job) = &self.clone_job {
            self.show_message(fmt(
                self.locale,
                "clone already in progress: {}",
                &[&job.url],
            ));
            return;
        }
        let repos_root = self.config.repos_root.clone();
        let (sender, receiver) = mpsc::channel();
        let worker_url = url.clone();
        tokio::task::spawn_blocking(move || {
            let result =
                crate::clone::clone_repo(&worker_url, &repos_root, branch.as_deref(), None)
                    .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.clone_job = Some(CloneJob { url, receiver });
        self.show_message(t(self.locale, "cloning repository"));
    }

    pub(in crate::ui) fn drain_clone_events(&mut self) {
        let result = match self.clone_job.as_ref().map(|job| job.receiver.try_recv()) {
            Some(Ok(result)) => Some(result),
            Some(Err(TryRecvError::Disconnected)) => {
                Some(Err(
                    t(self.locale, "clone worker terminated unexpectedly").to_string()
                ))
            }
            Some(Err(TryRecvError::Empty)) | None => None,
        };
        let Some(result) = result else {
            return;
        };
        self.clone_job = None;
        match result {
            Ok(path) => {
                let previous_len = self.registry.repos.len();
                match crate::registry::Registry::locked_add_pinned(&path)
                    .and_then(|_| self.registry.add_pinned(&path).map(|_| ()))
                {
                    Ok(()) => {
                        if self.registry.repos.len() > previous_len {
                            self.expanded.push(true);
                        }
                        self.show_message(fmt(
                            self.locale,
                            "repository added: {}",
                            &[&path.display().to_string()],
                        ));
                    }
                    Err(error) => self.show_message(error.to_string()),
                }
            }
            Err(error) => self.show_message(error),
        }
    }
}
