use std::{
    collections::HashMap,
    panic::{self, AssertUnwindSafe},
    time::{Duration, Instant},
};

use crate::dropr;

use super::super::App;

const REFRESH_STALE_AFTER: Duration = Duration::from_secs(30);

fn refresh_is_current(in_flight: &mut HashMap<String, Instant>, workspace_id: &str) -> bool {
    let now = Instant::now();
    let is_current = in_flight
        .get(workspace_id)
        .is_some_and(|started| now.saturating_duration_since(*started) < REFRESH_STALE_AFTER);
    if !is_current {
        in_flight.remove(workspace_id);
    }
    is_current
}

impl App {
    pub(in crate::ui) fn refresh_dropr_tasks(&mut self) {
        self.ingest_dropr_tasks();
        let workspace_ids = self
            .registry
            .repos
            .iter()
            .filter_map(|repo| repo.dropr.as_ref().map(|workspace| workspace.id.clone()))
            .collect::<Vec<_>>();
        for workspace_id in workspace_ids {
            self.schedule_dropr_tasks(workspace_id);
        }
    }

    pub(in crate::ui) fn refresh_repo_dropr_tasks(&mut self, repo_index: usize) -> bool {
        self.ingest_dropr_tasks();
        let Some(workspace_id) = self.registry.repos[repo_index]
            .dropr
            .as_ref()
            .map(|workspace| workspace.id.clone())
        else {
            return false;
        };
        self.schedule_dropr_tasks(workspace_id);
        true
    }

    fn schedule_dropr_tasks(&mut self, workspace_id: String) {
        if refresh_is_current(&mut self.dropr_task_refresh.in_flight, &workspace_id) {
            return;
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
