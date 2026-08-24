use std::{
    collections::{HashMap, HashSet},
    panic::{self, AssertUnwindSafe},
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

use crate::{
    dropr,
    dropr::DroprTaskFetch,
    locale::{Locale, t},
    model::RepoNode,
};

use super::super::App;
use super::dropr_overlay::OverlayStatus;

const REFRESH_STALE_AFTER: Duration = Duration::from_secs(30);

// A fetch that cannot finish inside the stale window has its answer thrown
// away, so the budget it runs under has to be the shorter of the two.
const _: () = assert!(dropr::FETCH_BUDGET.as_secs() < REFRESH_STALE_AFTER.as_secs());

/// What the pane says when a reload the operator asked for never came back.
/// Deliberately does not name the window: the constant above owns that number.
const ABANDONED_RELOAD: &str = "a manual reload gave up before it answered; nothing was re-checked";

type DroprTaskResult = (String, Instant, DroprTaskFetch);

pub(in crate::ui) struct DroprTaskRefresh {
    sender: Sender<DroprTaskResult>,
    receiver: Receiver<DroprTaskResult>,
    in_flight: HashMap<String, Instant>,
    manual: HashSet<String>,
}

impl DroprTaskRefresh {
    pub(in crate::ui) fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            in_flight: HashMap::new(),
            manual: HashSet::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DroprTaskReload {
    Running,
    Failed,
    /// The overlay loaded and matched no repository: nothing is linked.
    NoLinkedWorkspaces,
    /// No overlay load has completed yet, so linkage is simply unknown.
    OverlayPending,
    /// The `dropr` CLI is missing or its workspace listing keeps failing.
    OverlayUnavailable,
    /// Overlay loading is switched off, so links are never resolved.
    OverlayDisabled,
    /// Every linked repo's workspace is `virtual`: dropr knows the repository
    /// but nobody made a board for it yet, so there is nothing to fetch.
    NoMaterialisedWorkspaces,
}

/// Why no workspace is available to refresh. The distinction matters: reporting
/// an unloaded overlay as "no dropr-linked repos" sends the operator to check a
/// linkage that was never broken.
fn no_workspace_reason(overlay_enabled: bool, status: OverlayStatus) -> DroprTaskReload {
    if !overlay_enabled {
        return DroprTaskReload::OverlayDisabled;
    }
    match status {
        OverlayStatus::Pending => DroprTaskReload::OverlayPending,
        OverlayStatus::Loaded => DroprTaskReload::NoLinkedWorkspaces,
        OverlayStatus::Unavailable => DroprTaskReload::OverlayUnavailable,
    }
}

/// Whether any registered repo resolved to a dropr workspace at all, virtual
/// or materialised. Kept apart from [`materialised_workspace_ids`] so "no
/// linkage" and "linked, but no board yet" report as the distinct states they
/// are.
fn any_workspace_linked(repos: &[RepoNode]) -> bool {
    repos.iter().any(|repo| repo.dropr.is_some())
}

/// The workspace ids worth asking dropr for tasks: the materialised links
/// among the registered repos. A `virtual` workspace is a placeholder dropr
/// lists for a repository it knows about but never created a board for, so
/// `task_list` against it can only answer "not found" (dropr:516).
fn materialised_workspace_ids(repos: &[RepoNode]) -> Vec<String> {
    repos
        .iter()
        .filter_map(|repo| repo.dropr.as_ref())
        .filter(|workspace| workspace.is_materialised())
        .map(|workspace| workspace.id.clone())
        .collect()
}

fn refresh_is_fresh(started: Instant, now: Instant) -> bool {
    now.saturating_duration_since(started) < REFRESH_STALE_AFTER
}

/// Drops refreshes that outlived the stale window, and names the manual ones it
/// dropped. A background sweep can be abandoned quietly, but an operator who
/// pressed `r` is waiting on an answer and has to be told none is coming.
fn expire_stale_refreshes(refresh: &mut DroprTaskRefresh, now: Instant) -> Vec<String> {
    let abandoned = refresh
        .in_flight
        .iter()
        .filter(|(workspace_id, started)| {
            !refresh_is_fresh(**started, now) && refresh.manual.contains(*workspace_id)
        })
        .map(|(workspace_id, _)| workspace_id.clone())
        .collect();
    refresh
        .in_flight
        .retain(|_, started| refresh_is_fresh(*started, now));
    let in_flight = &refresh.in_flight;
    refresh
        .manual
        .retain(|workspace_id| in_flight.contains_key(workspace_id));
    abandoned
}

fn track_refresh(refresh: &mut DroprTaskRefresh, workspace_id: &str, manual: bool) -> bool {
    let now = Instant::now();
    let is_current = refresh
        .in_flight
        .get(workspace_id)
        .is_some_and(|started| refresh_is_fresh(*started, now));
    if !is_current {
        refresh.in_flight.remove(workspace_id);
        refresh.manual.remove(workspace_id);
    }
    if manual {
        refresh.manual.insert(workspace_id.to_owned());
    }
    is_current
}

/// Whether a fetch for `workspace_id` is currently running, manual or not.
/// Unlike [`refresh_visible`], this does not gate on `manual`: the summary
/// panel's "fetching" message (dropr:543) has to cover the very first,
/// background-triggered fetch a newly linked workspace gets, not just a
/// reload the operator asked for.
fn refresh_running(refresh: &DroprTaskRefresh, workspace_id: &str) -> bool {
    let now = Instant::now();
    refresh
        .in_flight
        .get(workspace_id)
        .is_some_and(|started| refresh_is_fresh(*started, now))
}

fn refresh_visible(refresh: &DroprTaskRefresh, workspace_id: &str) -> bool {
    refresh.manual.contains(workspace_id) && refresh_running(refresh, workspace_id)
}

/// Installs a fetch result, failures included.
///
/// A failed fetch replaces the old rows rather than preserving them: keeping
/// them would leave the pane showing a list nothing re-checked, with no sign
/// that the check failed. The fetch result carries its own explanation, so the
/// panel has something to show in their place.
fn apply_fetched_tasks(current: &mut DroprTaskFetch, fetched: DroprTaskFetch) {
    *current = fetched;
}

/// Records that a manual reload was abandoned, on the pane the operator is
/// looking at. Repeated sweeps must not stack the same line.
fn note_abandoned_reload(current: &mut DroprTaskFetch, locale: Locale) {
    let message = t(locale, ABANDONED_RELOAD);
    if !current.problems.iter().any(|line| line == message) {
        current.problems.push(message.to_owned());
    }
}

impl App {
    pub(super) fn refresh_dropr_tasks(&mut self, manual: bool) -> DroprTaskReload {
        self.ingest_dropr_tasks();
        if !any_workspace_linked(&self.registry.repos) {
            return no_workspace_reason(
                self.config.dropr_overlay,
                self.background_refresh.dropr_overlay_status,
            );
        }
        let workspace_ids = materialised_workspace_ids(&self.registry.repos);
        if workspace_ids.is_empty() {
            return DroprTaskReload::NoMaterialisedWorkspaces;
        }
        let mut any_running = false;
        for workspace_id in workspace_ids {
            any_running |= self.schedule_dropr_tasks(workspace_id, manual);
        }
        if any_running {
            DroprTaskReload::Running
        } else {
            DroprTaskReload::Failed
        }
    }

    pub(in crate::ui) fn dropr_refresh_in_flight(&self, workspace_id: &str) -> bool {
        refresh_visible(&self.dropr_task_refresh, workspace_id)
    }

    /// Whether the INFO pane's task list for `workspace_id` is waiting on an
    /// outstanding fetch — including the first, background-triggered one a
    /// freshly linked workspace gets, which [`dropr_refresh_in_flight`]
    /// (manual-only, used by the repo row's glyph) does not cover.
    pub(in crate::ui) fn dropr_task_fetch_running(&self, workspace_id: &str) -> bool {
        refresh_running(&self.dropr_task_refresh, workspace_id)
    }

    pub(super) fn schedule_dropr_tasks(&mut self, workspace_id: String, manual: bool) -> bool {
        if track_refresh(&mut self.dropr_task_refresh, &workspace_id, manual) {
            return true;
        }
        let started = Instant::now();
        self.dropr_task_refresh
            .in_flight
            .insert(workspace_id.clone(), started);
        let sender = self.dropr_task_refresh.sender.clone();
        let worker_workspace_id = workspace_id.clone();
        let locale = self.locale;
        let spawn_result = std::thread::Builder::new()
            .name("dropr-task-refresh".into())
            .spawn(move || {
                let fetch = panic::catch_unwind(AssertUnwindSafe(|| {
                    dropr::fetch_repo_tasks(&worker_workspace_id)
                }))
                .unwrap_or_else(|_| {
                    DroprTaskFetch::failed(t(locale, "the dropr task fetch panicked"))
                });
                let _ = sender.send((worker_workspace_id, started, fetch));
            });
        if spawn_result.is_err()
            && self.dropr_task_refresh.in_flight.get(&workspace_id) == Some(&started)
        {
            self.dropr_task_refresh.in_flight.remove(&workspace_id);
            self.dropr_task_refresh.manual.remove(&workspace_id);
        }
        spawn_result.is_ok()
    }

    fn ingest_dropr_tasks(&mut self) {
        let locale = self.locale;
        for workspace_id in expire_stale_refreshes(&mut self.dropr_task_refresh, Instant::now()) {
            for repo in Self::repos_of(&mut self.registry.repos, &workspace_id) {
                note_abandoned_reload(&mut repo.dropr_tasks, locale);
            }
        }
        while let Ok((workspace_id, started, fetch)) = self.dropr_task_refresh.receiver.try_recv() {
            if self.dropr_task_refresh.in_flight.get(&workspace_id) != Some(&started) {
                continue;
            }
            self.dropr_task_refresh.in_flight.remove(&workspace_id);
            self.dropr_task_refresh.manual.remove(&workspace_id);
            for repo in Self::repos_of(&mut self.registry.repos, &workspace_id) {
                apply_fetched_tasks(&mut repo.dropr_tasks, fetch.clone());
            }
        }
    }

    /// The repos linked to one dropr workspace. More than one repo can share a
    /// workspace, so a fetch result lands on every match.
    fn repos_of<'a>(
        repos: &'a mut [RepoNode],
        workspace_id: &'a str,
    ) -> impl Iterator<Item = &'a mut RepoNode> {
        repos.iter_mut().filter(move |repo| {
            repo.dropr
                .as_ref()
                .is_some_and(|workspace| workspace.id == workspace_id)
        })
    }
}

#[cfg(test)]
#[path = "dropr_tasks_tests.rs"]
mod tests;
