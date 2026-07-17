use std::path::Path;

use crate::model::{AgentNode, RepoNode};

pub(super) fn is_slot_worktree(
    path: &Path,
    branch: Option<&str>,
    tracked_agents: &[AgentNode],
) -> bool {
    tracked_agents
        .iter()
        .any(|tracked| matches_slot(path, branch, tracked))
}

pub(super) fn prune_slot_agents(repo: &mut RepoNode) {
    let tracked_agents = repo.agents.clone();
    repo.agents.retain(|candidate| {
        !tracked_agents
            .iter()
            .any(|tracked| matches_slot(&candidate.worktree_path, Some(&candidate.branch), tracked))
    });
}

fn matches_slot(path: &Path, branch: Option<&str>, tracked: &AgentNode) -> bool {
    branch.is_some_and(|candidate| branch_is_slot(candidate, &tracked.branch))
        || directory_is_slot(path, &tracked.worktree_path)
}

fn branch_is_slot(candidate: &str, tracked: &str) -> bool {
    candidate
        .strip_prefix(tracked)
        .and_then(|suffix| suffix.strip_prefix("-slot-"))
        .is_some_and(is_nonempty_digits)
}

fn directory_is_slot(candidate: &Path, tracked: &Path) -> bool {
    let (Some(candidate_parent), Some(tracked_parent)) = (candidate.parent(), tracked.parent())
    else {
        return false;
    };
    if super::discovery::path_key(candidate_parent) != super::discovery::path_key(tracked_parent) {
        return false;
    }
    let Some(candidate) = candidate.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(tracked) = tracked.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some((prefix, random)) = tracked.rsplit_once('_') else {
        return false;
    };
    if random.is_empty() || !prefix_is_managed_task(prefix) {
        return false;
    }
    candidate
        .strip_prefix(prefix)
        .and_then(|suffix| suffix.strip_prefix("_slot"))
        .is_some_and(is_nonempty_digits)
}

fn prefix_is_managed_task(prefix: &str) -> bool {
    prefix.rsplit_once("_task-").is_some_and(|(repo, task)| {
        let valid_task = task.split_once('-').map_or_else(
            || is_nonempty_digits(task),
            |(task_id, slug)| is_nonempty_digits(task_id) && !slug.is_empty(),
        );
        !repo.is_empty() && valid_task
    })
}

fn is_nonempty_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agent, config::Config, model::RepoNode};

    #[test]
    fn detects_anchored_branch_and_directory_slots() {
        let owner = agent("owner", "/wt/dropr_task-749_gOQmxo", "dropr/task-749");

        assert!(is_slot_worktree(
            Path::new("/wt/anything"),
            Some("dropr/task-749-slot-750"),
            std::slice::from_ref(&owner),
        ));
        assert!(is_slot_worktree(
            Path::new("/wt/dropr_task-749_slot754"),
            Some("unrelated"),
            &[owner],
        ));
    }

    #[test]
    fn detects_old_and_slugged_managed_task_directory_slots() {
        for owner_path in [
            "/wt/dropr_task-749_gOQmxo",
            "/wt/dropr_task-749-fix-chief_gOQmxo",
        ] {
            let owner = agent("owner", owner_path, "dropr/task-749-fix-chief");
            let prefix = owner_path.rsplit_once('_').unwrap().0;
            let slot_path = format!("{prefix}_slot750");

            assert!(is_slot_worktree(
                Path::new(&slot_path),
                None,
                std::slice::from_ref(&owner),
            ));
            assert!(is_slot_worktree(
                Path::new("/wt/anything"),
                Some("dropr/task-749-fix-chief-slot-750"),
                &[owner],
            ));
        }
    }

    #[test]
    fn rejects_slot_near_misses() {
        let owner = agent("owner", "/wt/repo_task-1_random", "feature");

        for branch in [
            "feature-slotmachine",
            "feature-slot-x",
            "feature-slot-1-extra",
            "feature-slot-",
        ] {
            assert!(!is_slot_worktree(
                Path::new("/wt/unrelated"),
                Some(branch),
                std::slice::from_ref(&owner),
            ));
        }
        for path in [
            "/wt/repo_task-1_slotx",
            "/wt/repo_task-1_slot1-extra",
            "/wt/repo_task-1_slot",
        ] {
            assert!(!is_slot_worktree(
                Path::new(path),
                None,
                std::slice::from_ref(&owner),
            ));
        }
    }

    #[test]
    fn user_suffix_does_not_make_directory_a_slot_owner() {
        let owner = agent("owner", "/wt/acme_custom", "feature");

        assert!(!is_slot_worktree(
            Path::new("/wt/acme_slot7"),
            None,
            &[owner],
        ));
    }

    #[test]
    fn directory_slot_must_be_a_sibling_of_its_owner() {
        let owner = agent("owner", "/wt/repo_task-1_random", "feature");

        assert!(!is_slot_worktree(
            Path::new("/wt/group/repo_task-1_slot7"),
            None,
            &[owner],
        ));
    }

    #[test]
    fn slot_like_random_suffix_still_owns_sibling_slot() {
        let owner = agent("owner", "/wt/repo_task-1_slot12", "feature");

        assert!(is_slot_worktree(
            Path::new("/wt/repo_task-1_slot13"),
            None,
            &[owner],
        ));
    }

    #[test]
    fn prunes_adopted_slot_without_pruning_owner_in_either_order() {
        for ids in [["slot", "owner"], ["owner", "slot"]] {
            let mut repo = repo();
            repo.agents = ids
                .into_iter()
                .map(|id| match id {
                    "slot" => agent("slot", "/wt/repo_task-1_slot2", "repo/task-1-slot-2"),
                    _ => agent("owner", "/wt/repo_task-1_random", "repo/task-1"),
                })
                .collect();

            prune_slot_agents(&mut repo);

            assert_eq!(repo.agents.len(), 1);
            assert_eq!(repo.agents[0].id, "owner");
        }
    }

    #[test]
    fn keeps_normal_and_user_created_agents() {
        let mut repo = repo();
        repo.agents = vec![
            agent("normal", "/wt/repo_task-2_other", "repo/task-2"),
            agent("user", "/wt/my-worktree", "feature-slotmachine"),
        ];

        prune_slot_agents(&mut repo);

        let ids: Vec<_> = repo.agents.iter().map(|agent| agent.id.as_str()).collect();
        assert_eq!(ids, ["normal", "user"]);
    }

    #[test]
    fn prunes_orphaned_slot_by_self_match_but_keeps_lone_owner() {
        let mut repo = repo();
        repo.agents = vec![
            agent("slot", "/wt/repo_task-1_slot2", "unrelated"),
            agent("owner", "/wt/repo_task-2_random", "unrelated"),
        ];

        prune_slot_agents(&mut repo);

        assert_eq!(repo.agents.len(), 1);
        assert_eq!(repo.agents[0].id, "owner");
    }

    fn agent(id: &str, path: &str, branch: &str) -> AgentNode {
        let repo = repo();
        let mut agent = agent::adopt_worktree(
            &repo,
            &Config::default(),
            path.into(),
            Some(branch.into()),
            None,
            None,
            None,
        );
        agent.id = id.into();
        agent
    }

    fn repo() -> RepoNode {
        RepoNode {
            path: "/repo".into(),
            name: "repo".into(),
            remote_url: None,
            pinned: false,
            agents: Vec::new(),
            dropr: None,
            dropr_tasks: Vec::new(),
            main_status: None,
            main_last_capture: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
        }
    }
}
