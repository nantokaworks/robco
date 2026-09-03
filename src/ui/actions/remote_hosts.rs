use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::model::Status;
use crate::{
    config::{Config, HostConfig},
    model::{HostLabel, RepoNode},
    overseer::discord_channels::DiscordChannels,
};

use super::{background_support::carry_runtime, discovery::path_key};
use crate::ui::{
    App, DISCOVERY_INTERVAL,
    backend::{Backend, RemoteBackend},
};

#[derive(Default)]
struct HostSnapshot {
    repos: Vec<RepoNode>,
    control_status: Option<Status>,
    discord_channels: DiscordChannels,
    daemon_alive: bool,
    error: Option<String>,
    backend: Option<Arc<RemoteBackend>>,
    generation: u64,
}

#[derive(Clone, Default)]
pub(in crate::ui) struct HostView {
    pub(in crate::ui) connection: HostConnection,
    pub(in crate::ui) error: Option<String>,
    pub(in crate::ui) control_status: Option<Status>,
    pub(in crate::ui) discord_channels: DiscordChannels,
    pub(in crate::ui) daemon_alive: bool,
}

impl HostView {
    fn from_snapshot(snapshot: &HostSnapshot) -> Self {
        let connection = if snapshot.error.is_some() {
            HostConnection::Failed
        } else if snapshot.generation == 0 {
            HostConnection::Connecting
        } else {
            HostConnection::Connected
        };
        Self {
            connection,
            error: snapshot.error.clone(),
            control_status: snapshot.control_status,
            discord_channels: snapshot.discord_channels.clone(),
            daemon_alive: snapshot.daemon_alive,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum HostConnection {
    #[default]
    Connecting,
    Connected,
    Failed,
}

pub(in crate::ui) struct HostSlot {
    pub(in crate::ui) label: HostLabel,
    snapshot: Arc<Mutex<HostSnapshot>>,
    applied_generation: u64,
}

impl HostSlot {
    fn spawn(host: HostConfig, config: Config) -> Self {
        let label = HostLabel {
            name: host.name.unwrap_or_else(|| host.ssh.clone()),
            ssh: host.ssh,
        };
        let snapshot = Arc::new(Mutex::new(HostSnapshot::default()));
        let thread_label = label.clone();
        let thread_snapshot = Arc::clone(&snapshot);
        let _ = std::thread::Builder::new()
            .name(format!("ui-remote-{}", label.name))
            .spawn(move || poll_host(thread_label, config, thread_snapshot));
        Self {
            label,
            snapshot,
            applied_generation: 0,
        }
    }

    #[cfg(test)]
    fn snapshot_view(&self) -> HostView {
        let snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        HostView::from_snapshot(&snapshot)
    }

    #[cfg(test)]
    pub(in crate::ui) fn connection_and_error(&self) -> (HostConnection, Option<String>) {
        let view = self.snapshot_view();
        (view.connection, view.error)
    }

    pub(in crate::ui) fn backend(&self) -> Option<Arc<RemoteBackend>> {
        self.snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .backend
            .clone()
    }
}

fn poll_host(label: HostLabel, config: Config, cell: Arc<Mutex<HostSnapshot>>) {
    loop {
        match RemoteBackend::connect(&label.ssh) {
            Ok(backend) => {
                let backend = Arc::new(backend);
                let mut snapshot = cell
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                snapshot.backend = Some(Arc::clone(&backend));
                drop(snapshot);
                loop {
                    match backend.initial_snapshot() {
                        Ok((registry, _)) => {
                            let status =
                                backend.capture_status(registry, &config, &Default::default());
                            let overseer = status.overseer.snapshot;
                            publish(
                                &cell,
                                &label,
                                status.repos,
                                overseer.control_status,
                                overseer.discord_channels,
                                overseer.daemon_alive,
                                None,
                            );
                        }
                        Err(error) => {
                            publish_error(&cell, error.to_string());
                            break;
                        }
                    }
                    std::thread::sleep(DISCOVERY_INTERVAL);
                }
            }
            Err(error) => publish_error(&cell, error.to_string()),
        }
        std::thread::sleep(DISCOVERY_INTERVAL);
    }
}

fn publish(
    cell: &Mutex<HostSnapshot>,
    label: &HostLabel,
    mut repos: Vec<RepoNode>,
    control_status: Option<Status>,
    discord_channels: DiscordChannels,
    daemon_alive: bool,
    error: Option<String>,
) {
    for repo in &mut repos {
        repo.host = Some(label.clone());
    }
    let mut snapshot = cell
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    carry_runtime(&snapshot.repos, &mut repos, true);
    snapshot.repos = repos;
    snapshot.control_status = control_status;
    snapshot.discord_channels = discord_channels;
    snapshot.daemon_alive = daemon_alive;
    snapshot.error = error;
    snapshot.generation = snapshot.generation.wrapping_add(1);
}

fn publish_error(cell: &Mutex<HostSnapshot>, error: String) {
    let mut snapshot = cell
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    snapshot.error = Some(error);
    snapshot.generation = snapshot.generation.wrapping_add(1);
}

impl App {
    pub(in crate::ui) fn repo_host_key(&self, repo: usize) -> &str {
        self.registry.repos[repo]
            .host
            .as_ref()
            .map(|host| host.ssh.as_str())
            .unwrap_or("local")
    }

    pub(in crate::ui) fn start_remote_hosts(&mut self) {
        self.hosts = self
            .config
            .hosts
            .clone()
            .into_iter()
            .map(|host| HostSlot::spawn(host, self.config.clone()))
            .collect();
        self.host_views = vec![HostView::default(); self.hosts.len()];
    }

    pub(in crate::ui) fn connect_host(&mut self, ssh: String) {
        self.hosts.push(HostSlot::spawn(
            HostConfig { ssh, name: None },
            self.config.clone(),
        ));
        self.host_views.push(HostView::default());
    }

    pub(in crate::ui) fn host_view(&self, host: usize) -> Option<&HostView> {
        self.host_views.get(host)
    }

    pub(in crate::ui) fn ingest_remote_hosts(&mut self) {
        let selected = self.selected_item().map(|item| self.item_key(item));
        let expanded = expanded_map(&self.registry.repos, &self.expanded);
        let mut changed = false;
        self.host_views.truncate(self.hosts.len());
        for (host, slot) in self.hosts.iter_mut().enumerate() {
            let snapshot = slot
                .snapshot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if snapshot.generation == slot.applied_generation && self.host_views.get(host).is_some()
            {
                continue;
            }
            let view = HostView::from_snapshot(&snapshot);
            self.registry
                .repos
                .retain(|repo| repo.host.as_ref() != Some(&slot.label));
            self.registry.repos.extend(snapshot.repos.clone());
            slot.applied_generation = snapshot.generation;
            if let Some(current) = self.host_views.get_mut(host) {
                *current = view;
            } else {
                self.host_views.push(view);
            }
            changed = true;
        }
        if changed {
            self.expanded = self
                .registry
                .repos
                .iter()
                .map(|repo| expanded.get(&repo_key(repo)).copied().unwrap_or(true))
                .collect();
            self.restore_selection(selected);
        }
    }

    pub(in crate::ui) fn remote_client_for_repo(
        &self,
        repo: usize,
    ) -> Option<crate::remote::RemoteClient> {
        let host = self.registry.repos.get(repo)?.host.as_ref()?;
        self.hosts
            .iter()
            .find(|slot| slot.label == *host)?
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .backend
            .as_ref()
            .map(|backend| backend.client())
    }
}

fn repo_key(repo: &RepoNode) -> (Option<String>, String) {
    (
        repo.host.as_ref().map(|host| host.ssh.clone()),
        path_key(&repo.path),
    )
}

fn expanded_map(repos: &[RepoNode], expanded: &[bool]) -> HashMap<(Option<String>, String), bool> {
    repos
        .iter()
        .zip(expanded)
        .map(|(repo, value)| (repo_key(repo), *value))
        .collect()
}

#[cfg(test)]
#[path = "remote_hosts_test_support.rs"]
mod test_support;
#[cfg(test)]
#[path = "remote_hosts_tests.rs"]
mod tests;
