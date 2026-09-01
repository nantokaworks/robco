use std::path::PathBuf;

use chrono::Local;

use super::*;
use crate::model::Status;

#[test]
fn agent_order_nests_children_and_keeps_cycles_visible() {
    fn agent(id: &str, parent: Option<&str>) -> AgentNode {
        let now = Local::now();
        AgentNode {
            id: id.into(),
            parent_agent_id: parent.map(str::to_string),
            title: id.into(),
            task_number: None,
            worktree_path: PathBuf::from(id),
            branch: id.into(),
            base_commit: String::new(),
            program: String::new(),
            spawned_by_version: None,
            claude_session_id: None,
            profile: None,
            tmux_session: id.into(),
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
            mcp_active: false,
            pane_pid: None,
            tracked_command: None,
            subagents: Vec::new(),
            children: Vec::new(),
        }
    }
    let agents = vec![
        agent("parent", None),
        agent("other", None),
        agent("child", Some("parent")),
    ];
    assert_eq!(agent_order(&agents), vec![(0, 0), (2, 1), (1, 0)]);

    let cycle = vec![agent("a", Some("b")), agent("b", Some("a"))];
    assert_eq!(agent_order(&cycle), vec![(0, 0), (1, 1)]);

    let self_parent = vec![agent("self", Some("self")), agent("child", Some("self"))];
    assert_eq!(agent_order(&self_parent), vec![(0, 0), (1, 1)]);
}

#[test]
fn agent_rows_mark_last_siblings_and_ancestor_guides() {
    fn agent(id: &str, parent: Option<&str>) -> AgentNode {
        let now = Local::now();
        AgentNode {
            id: id.into(),
            parent_agent_id: parent.map(str::to_string),
            title: id.into(),
            task_number: None,
            worktree_path: PathBuf::from(id),
            branch: id.into(),
            base_commit: String::new(),
            program: String::new(),
            spawned_by_version: None,
            claude_session_id: None,
            profile: None,
            tmux_session: id.into(),
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
            mcp_active: false,
            pane_pid: None,
            tracked_command: None,
            subagents: Vec::new(),
            children: Vec::new(),
        }
    }

    // "parent" has a later root sibling "other", so its own row is not
    // last and carries no ancestors; "child" is parent's only child, so
    // it is last, but its ancestor ("parent") still has a later sibling,
    // so the guide over "child"'s row must continue.
    let agents = vec![
        agent("parent", None),
        agent("other", None),
        agent("child", Some("parent")),
    ];
    let rows = agent_rows(&agents);
    assert_eq!(
        rows,
        vec![
            AgentRow {
                index: 0,
                depth: 0,
                is_last: false,
                ancestor_continues: vec![],
            },
            AgentRow {
                index: 2,
                depth: 1,
                is_last: true,
                ancestor_continues: vec![true],
            },
            AgentRow {
                index: 1,
                depth: 0,
                is_last: true,
                ancestor_continues: vec![],
            },
        ]
    );

    // A deeper tree: "root" is the sole root (last), "mid-a" has a later
    // sibling "mid-b" (not last), and "leaf" hangs off "mid-a" alone
    // (last). "leaf"'s guide must be blank over "root"'s column (root has
    // no later sibling) but continue over "mid-a"'s column (mid-a does).
    let deeper = vec![
        agent("root", None),
        agent("mid-a", Some("root")),
        agent("mid-b", Some("root")),
        agent("leaf", Some("mid-a")),
    ];
    let rows = agent_rows(&deeper);
    let leaf = rows.iter().find(|row| row.index == 3).unwrap();
    assert_eq!(leaf.depth, 2);
    assert!(leaf.is_last);
    assert_eq!(leaf.ancestor_continues, vec![false, true]);
    let mid_a = rows.iter().find(|row| row.index == 1).unwrap();
    assert!(!mid_a.is_last);
    let mid_b = rows.iter().find(|row| row.index == 2).unwrap();
    assert!(mid_b.is_last);
}
