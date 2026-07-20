use std::{
    fs::OpenOptions,
    io::Write,
    sync::mpsc::{self, Sender},
    thread::JoinHandle,
};

use crate::{model::RepoNode, notify, overseer::logging, registry::Registry};

pub(super) fn clone_registry(registry: &Registry) -> Registry {
    Registry {
        version: registry.version,
        repos: registry.repos.clone(),
    }
}

pub(super) fn fingerprint(registry: &Registry) -> Vec<u8> {
    serde_json::to_vec(registry).unwrap_or_default()
}

pub(super) fn merge_status(current: &mut [RepoNode], refreshed: Vec<RepoNode>) {
    for repo in current {
        let Some(source) = refreshed.iter().find(|source| source.path == repo.path) else {
            continue;
        };
        copy_repo_status(source, repo);
        for tracked in &mut repo.agents {
            if let Some(source) = source.agents.iter().find(|source| source.id == tracked.id) {
                copy_agent_status(source, tracked);
            }
        }
    }
}

pub(super) fn carry_runtime(current: &[RepoNode], refreshed: &mut [RepoNode]) {
    for repo in refreshed {
        let Some(source) = current.iter().find(|source| source.path == repo.path) else {
            continue;
        };
        copy_repo_status(source, repo);
        repo.dropr_tasks.clone_from(&source.dropr_tasks);
        if repo.dropr.is_none() {
            repo.dropr.clone_from(&source.dropr);
        }
        for tracked in &mut repo.agents {
            if let Some(source) = source.agents.iter().find(|source| source.id == tracked.id) {
                copy_agent_status(source, tracked);
                tracked.merge_error.clone_from(&source.merge_error);
            }
        }
    }
}

pub(super) struct RegistrySaver {
    sender: Option<Sender<(Registry, Vec<u8>)>>,
    handle: Option<JoinHandle<()>>,
}

impl RegistrySaver {
    pub(super) fn new() -> Self {
        let (sender, receiver) = mpsc::channel::<(Registry, Vec<u8>)>();
        let handle = std::thread::Builder::new()
            .name("ui-registry-save".into())
            .spawn(move || {
                for (registry, base_fingerprint) in receiver {
                    let _ = Registry::locked_update(|stored| {
                        if fingerprint(stored) == base_fingerprint {
                            stored.version = registry.version;
                            stored.repos.clone_from(&registry.repos);
                        }
                    });
                }
            })
            .ok();
        Self {
            sender: handle.as_ref().map(|_| sender),
            handle,
        }
    }

    pub(super) fn save(&self, registry: Registry, base_fingerprint: Vec<u8>) {
        if let Some(sender) = &self.sender {
            let _ = sender.send((registry, base_fingerprint));
        }
    }

    pub(super) fn shutdown(&mut self) {
        self.sender.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn copy_repo_status(source: &RepoNode, target: &mut RepoNode) {
    target.main_status = source.main_status;
    target
        .main_last_capture
        .clone_from(&source.main_last_capture);
    target.main_last_change_at = source.main_last_change_at;
    target.main_shell_working = source.main_shell_working;
    target.main_pane_pid = source.main_pane_pid;
    target
        .main_tracked_command
        .clone_from(&source.main_tracked_command);
}

fn copy_agent_status(source: &crate::model::AgentNode, target: &mut crate::model::AgentNode) {
    target.status = source.status;
    target.worktree_missing = source.worktree_missing;
    target.last_capture.clone_from(&source.last_capture);
    target.last_change_at = source.last_change_at;
    target.last_auto_accept_at = source.last_auto_accept_at;
    target.shell_working = source.shell_working;
    target.pane_pid = source.pane_pid;
    target.tracked_command.clone_from(&source.tracked_command);
}

pub(super) fn notify_new_decisions(
    cursor: &mut logging::DigestCursor,
    stdout_tx: &Sender<String>,
    enabled: bool,
) {
    let Ok(digest) = cursor.read_digest() else {
        return;
    };
    if enabled && let Some(body) = digest {
        route_notification(stdout_tx, "robco", &body);
    }
}

fn route_notification(stdout_tx: &Sender<String>, title: &str, body: &str) {
    let payload = notify::osc777(title, body);
    let ttys = notify::attached_client_ttys();
    if ttys.is_empty() {
        let _ = stdout_tx.send(payload);
        return;
    }
    for tty in ttys {
        if let Ok(mut file) = OpenOptions::new().write(true).open(tty) {
            let _ = file.write_all(payload.as_bytes());
            let _ = file.flush();
        }
    }
}
