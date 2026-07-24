use std::time::SystemTime;

use crate::{
    model::RepoNode,
    subagents::{SubagentReader, SubagentStatus, read_allowed},
};

pub(super) fn ingest(
    repos: &mut [RepoNode],
    enabled: bool,
    reader: &dyn SubagentReader,
    now: SystemTime,
) {
    if !enabled {
        for repo in repos {
            repo.main_subagents_active = 0;
            for agent in &mut repo.agents {
                agent.subagents.clear();
            }
        }
        return;
    }

    for repo in repos {
        repo.main_subagents_active = repo.main_status.map_or(0, |_| {
            reader
                .read(&repo.path, None, now)
                .iter()
                .filter(|subagent| subagent.status == SubagentStatus::Running)
                .count()
        });
        for agent in &mut repo.agents {
            if !read_allowed(agent.status, &agent.worktree_path) {
                agent.subagents.clear();
            } else {
                agent.subagents = reader.read(
                    &agent.worktree_path,
                    agent.claude_session_id.as_deref(),
                    now,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        path::{Path, PathBuf},
        time::Duration,
    };

    use chrono::Local;

    use super::*;
    use crate::subagents::TaskSubagent;
    use crate::{
        config::Config,
        model::{AgentNode, Status},
        registry::Registry,
        subagents::claude::ClaudeSubagentReader,
        ui::App,
    };

    #[derive(Default)]
    struct FakeReader {
        reads: RefCell<Vec<(PathBuf, Option<String>)>>,
    }

    impl SubagentReader for FakeReader {
        fn read(
            &self,
            path: &Path,
            session_id: Option<&str>,
            now: SystemTime,
        ) -> Vec<TaskSubagent> {
            self.reads
                .borrow_mut()
                .push((path.to_path_buf(), session_id.map(str::to_string)));
            let statuses = if path == Path::new("/repo") {
                vec![SubagentStatus::Running, SubagentStatus::Done]
            } else {
                vec![SubagentStatus::Running]
            };
            statuses
                .into_iter()
                .enumerate()
                .map(|(index, status)| TaskSubagent {
                    id: format!("worker-{index}"),
                    agent_type: "Explore".into(),
                    description: "inspect".into(),
                    spawn_depth: 1,
                    started_at: now - Duration::from_secs(2),
                    last_activity_at: now,
                    status,
                })
                .collect()
        }
    }

    fn repo(path: &Path, worktree_path: &Path) -> RepoNode {
        let now = Local::now();
        RepoNode {
            path: path.into(),
            name: "repo".into(),
            remote_url: None,
            pinned: false,
            agents: vec![AgentNode {
                management: crate::model::ManagementMode::Manual,
                id: "agent".into(),
                parent_agent_id: None,
                title: "task".into(),
                worktree_path: worktree_path.into(),
                branch: "task".into(),
                base_commit: String::new(),
                program: "claude".into(),
                claude_session_id: Some("session-agent".into()),
                profile: None,
                tmux_session: "robco_repo_task".into(),
                created_at: now,
                updated_at: now,
                status: Status::Idle,
                worktree_missing: false,
                merge_error: None,
                last_capture: None,
                last_spinner: None,
                last_change_at: None,
                last_auto_accept_at: None,
                shell_working: false,
                pane_pid: None,
                tracked_command: None,
                subagents: Vec::new(),
                children: Vec::new(),
            }],
            dropr: None,
            dropr_tasks: Vec::new(),
            main_status: Some(Status::Idle),
            main_last_capture: None,
            main_last_spinner: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
        }
    }

    #[test]
    fn ingests_agents_and_counts_running_main_subagents() {
        let temp = tempfile::tempdir().unwrap();
        let repo_path = temp.path().join("repo");
        let worktree_path = temp.path().join("worktree");
        std::fs::create_dir_all(&repo_path).unwrap();
        std::fs::create_dir_all(&worktree_path).unwrap();
        let reader = FakeReader::default();
        let mut repos = vec![repo(&repo_path, &worktree_path)];

        ingest(&mut repos, true, &reader, SystemTime::now());

        assert_eq!(
            reader.reads.borrow().as_slice(),
            [
                (repo_path, None),
                (worktree_path, Some("session-agent".into()))
            ]
        );
        assert_eq!(repos[0].main_subagents_active, 1);
        assert_eq!(repos[0].agents[0].subagents.len(), 1);
    }

    #[test]
    fn absent_main_session_clears_count_without_reading_repo() {
        let temp = tempfile::tempdir().unwrap();
        let worktree_path = temp.path().join("worktree");
        std::fs::create_dir_all(&worktree_path).unwrap();
        let reader = FakeReader::default();
        let mut repos = vec![repo(temp.path(), &worktree_path)];
        repos[0].main_status = None;
        repos[0].main_subagents_active = 4;

        ingest(&mut repos, true, &reader, SystemTime::now());

        assert_eq!(
            reader.reads.borrow().as_slice(),
            [(worktree_path, Some("session-agent".into()))]
        );
        assert_eq!(repos[0].main_subagents_active, 0);
    }

    #[test]
    fn dead_or_missing_agents_clear_state_without_reads() {
        let temp = tempfile::tempdir().unwrap();
        let dead_path = temp.path().join("dead");
        std::fs::create_dir_all(&dead_path).unwrap();
        let missing_path = temp.path().join("missing");
        let reader = FakeReader::default();
        let now = SystemTime::now();
        let mut repo = repo(temp.path(), &dead_path);
        repo.main_status = None;
        repo.agents[0].status = Status::Dead;
        repo.agents[0].subagents = reader.read(Path::new("/fixture"), None, now);
        let mut missing_agent = repo.agents[0].clone();
        missing_agent.status = Status::Running;
        missing_agent.worktree_path = missing_path;
        repo.agents.push(missing_agent);
        reader.reads.borrow_mut().clear();

        ingest(std::slice::from_mut(&mut repo), true, &reader, now);

        assert!(reader.reads.borrow().is_empty());
        assert!(repo.agents.iter().all(|agent| agent.subagents.is_empty()));
    }

    #[test]
    fn disabled_indicator_clears_state_without_reads() {
        let reader = FakeReader::default();
        let now = SystemTime::now();
        let temp = tempfile::tempdir().unwrap();
        let mut repos = vec![repo(temp.path(), temp.path())];
        repos[0].main_subagents_active = 4;
        repos[0].agents[0].subagents = reader.read(Path::new("/fixture"), None, now);
        reader.reads.borrow_mut().clear();

        ingest(&mut repos, false, &reader, now);

        assert!(reader.reads.borrow().is_empty());
        assert_eq!(repos[0].main_subagents_active, 0);
        assert!(repos[0].agents[0].subagents.is_empty());
    }

    #[test]
    fn disabled_indicator_clears_state_when_discovery_fails() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            subagent_indicator: false,
            worktree_root: temp.path().into(),
            // Keep discovery inside the temp dir; the default repos_root points
            // at the real ~/.robco/repos, so a populated dev machine would
            // discover live repos, reorder the registry, and overwrite state.
            repos_root: temp.path().join("repos"),
            ..Default::default()
        };
        let registry = Registry {
            version: 1,
            repos: vec![repo(temp.path(), temp.path())],
        };
        let mut app = App::new(registry, config, temp.path().join("missing"));
        app.registry.repos[0].main_subagents_active = 4;
        app.registry.repos[0].agents[0].subagents =
            FakeReader::default().read(Path::new("/fixture"), None, SystemTime::now());

        ingest(
            &mut app.registry.repos,
            false,
            &ClaudeSubagentReader::default(),
            SystemTime::now(),
        );

        assert_eq!(app.registry.repos[0].main_subagents_active, 0);
        assert!(app.registry.repos[0].agents[0].subagents.is_empty());
    }
}
