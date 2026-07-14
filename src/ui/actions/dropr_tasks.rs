use std::{
    collections::HashMap,
    panic::{self, AssertUnwindSafe},
    time::{Duration, Instant},
};

use crate::dropr;

use super::super::App;

const REFRESH_STALE_AFTER: Duration = Duration::from_secs(30);

pub(super) enum DroprTaskReload {
    Running,
    Failed,
    NoLinkedWorkspaces,
}

fn refresh_is_fresh(started: Instant, now: Instant) -> bool {
    now.saturating_duration_since(started) < REFRESH_STALE_AFTER
}

fn refresh_is_current(in_flight: &mut HashMap<String, Instant>, workspace_id: &str) -> bool {
    let now = Instant::now();
    let is_current = in_flight
        .get(workspace_id)
        .is_some_and(|started| refresh_is_fresh(*started, now));
    if !is_current {
        in_flight.remove(workspace_id);
    }
    is_current
}

impl App {
    pub(super) fn refresh_dropr_tasks(&mut self) -> DroprTaskReload {
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
            any_running |= self.schedule_dropr_tasks(workspace_id);
        }
        if any_running {
            DroprTaskReload::Running
        } else {
            DroprTaskReload::Failed
        }
    }

    pub(in crate::ui) fn dropr_refresh_in_flight(&self, workspace_id: &str) -> bool {
        let now = Instant::now();
        self.dropr_task_refresh
            .in_flight
            .get(workspace_id)
            .is_some_and(|started| refresh_is_fresh(*started, now))
    }

    fn schedule_dropr_tasks(&mut self, workspace_id: String) -> bool {
        if refresh_is_current(&mut self.dropr_task_refresh.in_flight, &workspace_id) {
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
        }
        spawn_result.is_ok()
    }

    fn ingest_dropr_tasks(&mut self) {
        while let Ok((workspace_id, started, tasks)) = self.dropr_task_refresh.receiver.try_recv() {
            if self.dropr_task_refresh.in_flight.get(&workspace_id) != Some(&started) {
                continue;
            }
            self.dropr_task_refresh.in_flight.remove(&workspace_id);
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
    fn stale_refresh_is_expired() {
        let mut in_flight =
            HashMap::from([("workspace".to_owned(), Instant::now() - REFRESH_STALE_AFTER)]);

        assert!(!refresh_is_current(&mut in_flight, "workspace"));
        assert!(!in_flight.contains_key("workspace"));
    }
}
