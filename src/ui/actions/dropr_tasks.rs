use std::{
    panic::{self, AssertUnwindSafe},
    time::{Duration, Instant},
};

use crate::dropr;

use super::super::{App, DroprTaskRefresh};

const REFRESH_STALE_AFTER: Duration = Duration::from_secs(30);

pub(super) enum DroprTaskReload {
    Running,
    Failed,
    NoLinkedWorkspaces,
}

fn refresh_is_fresh(started: Instant, now: Instant) -> bool {
    now.saturating_duration_since(started) < REFRESH_STALE_AFTER
}

fn expire_stale_refreshes(refresh: &mut DroprTaskRefresh, now: Instant) {
    refresh
        .in_flight
        .retain(|_, started| refresh_is_fresh(*started, now));
    let in_flight = &refresh.in_flight;
    refresh
        .manual
        .retain(|workspace_id| in_flight.contains_key(workspace_id));
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

fn refresh_visible(refresh: &DroprTaskRefresh, workspace_id: &str) -> bool {
    let now = Instant::now();
    refresh.manual.contains(workspace_id)
        && refresh
            .in_flight
            .get(workspace_id)
            .is_some_and(|started| refresh_is_fresh(*started, now))
}

impl App {
    pub(super) fn refresh_dropr_tasks(&mut self, manual: bool) -> DroprTaskReload {
        self.ingest_dropr_tasks();
        let workspace_ids = self
            .registry
            .repos
            .iter()
            .filter_map(|repo| repo.dropr.as_ref().map(|workspace| workspace.id.clone()))
            .collect::<Vec<_>>();
        if workspace_ids.is_empty() {
            return DroprTaskReload::NoLinkedWorkspaces;
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

    fn schedule_dropr_tasks(&mut self, workspace_id: String, manual: bool) -> bool {
        if track_refresh(&mut self.dropr_task_refresh, &workspace_id, manual) {
            return true;
        }
        let started = Instant::now();
        self.dropr_task_refresh
            .in_flight
            .insert(workspace_id.clone(), started);
        let sender = self.dropr_task_refresh.sender.clone();
        let worker_workspace_id = workspace_id.clone();
        let spawn_result = std::thread::Builder::new()
            .name("dropr-task-refresh".into())
            .spawn(move || {
                let tasks = panic::catch_unwind(AssertUnwindSafe(|| {
                    dropr::fetch_ready_tasks(&worker_workspace_id)
                }))
                .ok()
                .flatten();
                let _ = sender.send((worker_workspace_id, started, tasks));
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
        expire_stale_refreshes(&mut self.dropr_task_refresh, Instant::now());
        while let Ok((workspace_id, started, tasks)) = self.dropr_task_refresh.receiver.try_recv() {
            if self.dropr_task_refresh.in_flight.get(&workspace_id) != Some(&started) {
                continue;
            }
            self.dropr_task_refresh.in_flight.remove(&workspace_id);
            self.dropr_task_refresh.manual.remove(&workspace_id);
            for repo in &mut self.registry.repos {
                if repo.dropr.as_ref().map(|workspace| &workspace.id) == Some(&workspace_id) {
                    repo.dropr_tasks = tasks.clone().unwrap_or_default();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_refresh_is_hidden_from_ui() {
        let mut refresh = DroprTaskRefresh::new();
        assert!(!track_refresh(&mut refresh, "workspace", false));
        refresh
            .in_flight
            .insert("workspace".to_owned(), Instant::now());

        assert!(!refresh_visible(&refresh, "workspace"));
    }

    #[test]
    fn manual_refresh_is_visible_to_ui() {
        let mut refresh = DroprTaskRefresh::new();
        assert!(!track_refresh(&mut refresh, "workspace", true));
        refresh
            .in_flight
            .insert("workspace".to_owned(), Instant::now());

        assert!(refresh_visible(&refresh, "workspace"));
    }

    #[test]
    fn stale_refresh_expires_manual_flag() {
        let mut refresh = DroprTaskRefresh::new();
        refresh
            .in_flight
            .insert("workspace".to_owned(), Instant::now() - REFRESH_STALE_AFTER);
        refresh.manual.insert("workspace".to_owned());

        assert!(!track_refresh(&mut refresh, "workspace", false));
        assert!(!refresh.in_flight.contains_key("workspace"));
        assert!(!refresh.manual.contains("workspace"));
    }

    #[test]
    fn sweep_expires_removed_workspace_refresh() {
        let now = Instant::now();
        let mut refresh = DroprTaskRefresh::new();
        refresh
            .in_flight
            .insert("removed-workspace".to_owned(), now - REFRESH_STALE_AFTER);
        refresh.in_flight.insert("linked-workspace".to_owned(), now);
        refresh.manual.insert("removed-workspace".to_owned());
        refresh.manual.insert("linked-workspace".to_owned());

        expire_stale_refreshes(&mut refresh, now);

        assert!(!refresh.in_flight.contains_key("removed-workspace"));
        assert!(!refresh.manual.contains("removed-workspace"));
        assert!(refresh.in_flight.contains_key("linked-workspace"));
        assert!(refresh.manual.contains("linked-workspace"));
    }

    #[test]
    fn manual_request_marks_in_flight_background_refresh() {
        let mut refresh = DroprTaskRefresh::new();
        refresh
            .in_flight
            .insert("workspace".to_owned(), Instant::now());

        assert!(track_refresh(&mut refresh, "workspace", true));
        assert!(refresh_visible(&refresh, "workspace"));
    }
}
