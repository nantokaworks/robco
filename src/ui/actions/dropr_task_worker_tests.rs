use std::{cell::RefCell, time::Duration};

use super::*;
use crate::{config::Profile, dropr::DroprTaskCandidate};

fn candidate(display_id: &str, id: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_string(),
        title: "Task".to_string(),
        description: None,
        priority: String::new(),
        status: "open".to_string(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
        id: id.to_string(),
        parent_task_id: None,
        child_count: 0,
    }
}

fn target_with(config: Config) -> TaskLaunchTarget {
    TaskLaunchTarget {
        repo: crate::ui::test_support::repo("/repo".into(), Vec::new()),
        config,
        workspace_id: "ws-1".to_string(),
        candidate: candidate("#546", "task-nanoid"),
        title: "#546 Task".to_string(),
    }
}

fn refusing_fetch(
    _workspace_id: &str,
    _parent_task_id: &str,
    _timeout: Duration,
) -> Vec<dropr::Subtask> {
    panic!("a genuinely childless task must not fetch over the network")
}

/// dropr:546's regression: before the fix, this call site passed `&[]` /
/// `&[]` to `launch` no matter what the profile configured, silently
/// dropping the permission flag, the env blocklist, and `SessionEnv` for
/// every worker started with the `n` key. This asserts the exact values
/// `run_launch_with` hands to `launch` match `tui_launch_env`'s resolution
/// for the same config — so a reversion back to empty slices at this call
/// site fails here, not only in a test of the resolver in isolation.
#[test]
fn n_key_launch_supplies_the_resolved_autonomous_args_and_env() {
    let mut config = Config {
        default_program: "claude".into(),
        profiles: vec![Profile {
            name: "claude".into(),
            program: "claude".into(),
            autonomous_args: vec!["--dangerously-skip-permissions".into()],
            model: None,
            backend: None,
        }],
        ..Config::default()
    };
    config.overseer.worker_env_blocklist = vec!["PATH".into()];
    let target = target_with(config.clone());
    let expected = tui_launch_env(&config);

    let captured = RefCell::new(None);
    let result = run_launch_with(&target, refusing_fetch, |request: DroprTaskLaunch| {
        *captured.borrow_mut() = Some((request.extra_args.to_vec(), request.extra_env.to_vec()));
        Err(LaunchError::DroprUnreachable)
    });

    assert!(matches!(result, Err(TaskLaunchFailure::DroprUnreachable)));
    assert_eq!(captured.into_inner(), Some(expected));
}

/// A worker started with the `n` key when the profile has no
/// `autonomous_args` still launches normally — `run_launch_with` must not
/// error just because both resolved slices are empty.
#[test]
fn n_key_launch_with_no_configured_args_still_launches() {
    let config = Config {
        default_program: "claude".into(),
        profiles: Vec::new(),
        ..Config::default()
    };
    let target = target_with(config);

    let result = run_launch_with(&target, refusing_fetch, |request: DroprTaskLaunch| {
        assert!(request.extra_args.is_empty());
        assert!(request.extra_env.is_empty());
        Err(LaunchError::DroprUnreachable)
    });

    assert!(matches!(result, Err(TaskLaunchFailure::DroprUnreachable)));
}
