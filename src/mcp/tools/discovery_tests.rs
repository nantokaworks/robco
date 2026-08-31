use super::*;
use std::path::Path;

#[test]
fn recognizes_nested_and_numbered_slot_worktrees() {
    let registry = crate::mcp::tools::tests::registry_with_agent("a1");
    let agent = &registry.repos[0].agents[0];
    assert!(matches_slot(
        Path::new("/elsewhere"),
        Some("task-slot-2"),
        agent
    ));
    assert!(path_is_strictly_inside(
        Path::new("/repo-wt/nested"),
        Path::new("/repo-wt")
    ));
}

#[test]
fn capture_registry_keeps_persisted_shape_and_adds_runtime_fields() {
    let registry = crate::mcp::tools::tests::registry_with_agent("a1");
    let value = registry_json(
        &registry,
        &Config::default(),
        None,
        &ClaudeSubagentReader::default(),
        SystemTime::now(),
    )
    .unwrap();
    assert_eq!(value["version"], 1);
    assert!(value["repos"][0]["agents"][0].get("children").is_some());
    assert!(
        value["repos"][0]["agents"][0]
            .get("subagents_active")
            .is_some()
    );
}
