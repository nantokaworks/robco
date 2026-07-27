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
fn detects_producer_branch_and_resolves_owner_from_sibling_directory() {
    let owner = agent("owner", "/wt/nex_task-384", "nex/task-384");
    let agents = vec![agent("other", "/wt/nex_task-300", "nex/task-300"), owner];
    let path = Path::new("/wt/nex_task-384_slot_snap");
    let branch = Some("slot/task-386-snap");

    assert!(is_slot_worktree(path, branch, &agents));
    assert_eq!(slot_owner(path, branch, &agents), Some(1));
    assert!(is_slot_worktree(
        Path::new("/wt/unrelated"),
        Some("slot/task-3-work"),
        &agents,
    ));
}

#[test]
fn producer_directory_uses_the_exact_competing_owner_prefix() {
    let agents = vec![
        agent("short", "/wt/nex_task-38", "nex/task-38"),
        agent("owner", "/wt/nex_task-384", "nex/task-384"),
    ];

    assert_eq!(
        slot_owner(
            Path::new("/wt/nex_task-384_slot_snap"),
            Some("slot/task-386-snap"),
            &agents,
        ),
        Some(1)
    );
}

#[test]
fn producer_directory_requires_managed_owner_and_valid_known_branch() {
    let feature = agent("feature", "/wt/feature", "feature");
    assert!(!is_slot_worktree(
        Path::new("/wt/feature_slot_cleanup"),
        Some("cleanup"),
        std::slice::from_ref(&feature),
    ));

    let owner = agent("owner", "/wt/nex_task-384", "nex/task-384");
    assert!(!is_slot_worktree(
        Path::new("/wt/nex_task-384_slot_snap"),
        Some("unrelated"),
        std::slice::from_ref(&owner),
    ));
    assert!(is_slot_worktree(
        Path::new("/wt/nex_task-384_slot_snap"),
        None,
        &[owner],
    ));
}

#[test]
fn false_positive_name_is_not_pruned() {
    let mut repo = repo();
    repo.agents = vec![
        agent("feature", "/wt/feature", "feature"),
        agent("cleanup", "/wt/feature_slot_cleanup", "cleanup"),
    ];

    prune_top_level_slot_agents(&mut repo);

    let ids: Vec<_> = repo.agents.iter().map(|agent| agent.id.as_str()).collect();
    assert_eq!(ids, ["feature", "cleanup"]);
}

#[test]
fn false_positive_name_is_not_attached_during_reconcile() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("worktrees");
    let owner_path = root.join("feature");
    let candidate_path = root.join("feature_slot_cleanup");
    std::fs::create_dir_all(&owner_path).unwrap();
    std::fs::create_dir_all(&candidate_path).unwrap();
    let mut repo = repo();
    repo.agents
        .push(agent("feature", owner_path.to_str().unwrap(), "feature"));
    let config = Config {
        worktree_root: root,
        ..Config::default()
    };
    let worktree = crate::git::Worktree {
        path: candidate_path,
        head: None,
        branch: Some("cleanup".into()),
    };

    let (added, children_changed) =
        super::super::children::reconcile(&mut repo, &config, vec![worktree]);

    assert!(!added);
    assert!(!children_changed);
    assert!(repo.agents[0].children.is_empty());
}

#[test]
fn detects_old_and_slugged_managed_task_directory_slots() {
    for owner_path in [
        "/wt/dropr_task-749_gOQmxo",
        "/wt/dropr_task-749-fix-overseer_gOQmxo",
    ] {
        let owner = agent("owner", owner_path, "dropr/task-749-fix-overseer");
        let prefix = owner_path.rsplit_once('_').unwrap().0;
        let slot_path = format!("{prefix}_slot750");

        assert!(is_slot_worktree(
            Path::new(&slot_path),
            None,
            std::slice::from_ref(&owner),
        ));
        assert!(is_slot_worktree(
            Path::new("/wt/anything"),
            Some("dropr/task-749-fix-overseer-slot-750"),
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

        prune_top_level_slot_agents(&mut repo);

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

    prune_top_level_slot_agents(&mut repo);

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

    prune_top_level_slot_agents(&mut repo);

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
        management: crate::model::ManagementMode::Auto,
        agents: Vec::new(),
        dropr: None,
        dropr_tasks: crate::dropr::DroprTaskFetch::default(),
        main_status: None,
        main_last_capture: None,
        main_last_spinner: None,
        main_last_change_at: None,
        main_shell_working: false,
        main_mcp_active: false,
        main_pane_pid: None,
        main_tracked_command: None,
        main_subagents_active: 0,
    }
}
