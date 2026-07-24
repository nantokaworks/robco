use super::*;

#[test]
fn manual_overseer_children_are_not_adopted() {
    let registry: Registry = serde_json::from_value(serde_json::json!({
        "version": 1,
        "repos": [{
            "path": "/repo",
            "name": "repo",
            "remote_url": null,
            "agents": [{
                "id": "manual-worker",
                "parent_agent_id": crate::overseer::OVERSEER_AGENT_ID,
                "management": "manual",
                "title": "#154",
                "worktree_path": "/repo/worker",
                "branch": "task-154",
                "base_commit": "",
                "program": "codex",
                "tmux_session": "robco_repo_task-154",
                "created_at": "2026-07-18T00:00:00+09:00",
                "updated_at": "2026-07-18T00:00:00+09:00"
            }]
        }]
    }))
    .unwrap();
    let mut ledger = Ledger::default();

    adopt_registry_children_from(&mut ledger, &registry);

    assert!(ledger.entries.is_empty());
}
