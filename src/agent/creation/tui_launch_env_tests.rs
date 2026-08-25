//! dropr:538 / dropr:546 — proof that the TUI creation paths
//! (`src/ui/input.rs`'s plain `create_agent`, and `src/ui/actions/dropr_task_worker.rs`'s
//! `n`-key launch, which calls this resolver directly rather than through
//! `create_agent`) resolve the profile's `autonomous_args`, the paired env
//! blocklist, and the operator's `SessionEnv` credential channel together.
//! Split from `creation/tests.rs` to keep that file under the 300-line
//! limit.

use super::*;

/// `PATH` is used as the blocklist probe because it is guaranteed present in
/// any process's environment, so a non-empty result proves the blocklist
/// actually ran rather than merely returning `Vec::new`.
#[test]
fn tui_launch_env_applies_the_blocklist_when_the_profile_has_args() {
    let mut config = Config {
        default_program: "claude".into(),
        profiles: vec![crate::config::Profile {
            name: "claude".into(),
            program: "claude".into(),
            autonomous_args: vec!["--dangerously-skip-permissions".into()],
            model: None,
            backend: None,
            clear_command: None,
        }],
        ..Config::default()
    };
    config.overseer.worker_env_blocklist = vec!["PATH".into()];

    let (args, env) = tui_launch_env(&config);

    assert_eq!(args, vec!["--dangerously-skip-permissions".to_string()]);
    assert!(
        env.iter().any(|(name, _)| name == "PATH"),
        "the permission flag must not travel without the env blocklist"
    );
}

/// The other half of the pairing: no configured `autonomous_args` means no
/// permission flag, and the blocklist must not run either — a worker with
/// neither still launches normally, not with a half-applied safety net.
#[test]
fn tui_launch_env_skips_the_blocklist_when_the_profile_has_no_args() {
    let mut config = Config {
        default_program: "claude".into(),
        profiles: vec![crate::config::Profile {
            name: "claude".into(),
            program: "claude".into(),
            autonomous_args: Vec::new(),
            model: None,
            backend: None,
            clear_command: None,
        }],
        ..Config::default()
    };
    config.overseer.worker_env_blocklist = vec!["PATH".into()];

    let (args, env) = tui_launch_env(&config);

    assert!(args.is_empty());
    assert!(env.is_empty());
}

/// A missing `claude` profile (`default_program_autonomous_args` finds no
/// matching profile and returns `Vec::new`) must degrade the same way an
/// explicit empty `autonomous_args` does, not error.
#[test]
fn tui_launch_env_with_no_matching_profile_is_empty_not_an_error() {
    let config = Config {
        default_program: "claude".into(),
        profiles: Vec::new(),
        ..Config::default()
    };

    let (args, env) = tui_launch_env(&config);

    assert!(args.is_empty());
    assert!(env.is_empty());
}

/// dropr:546 — the gap this task closes: `SessionEnv` must reach a TUI
/// launch even when the profile has no `autonomous_args` at all, since the
/// two channels are independent (an operator can configure a session
/// credential without ever enabling the permission flag). `session_env_file`
/// points at a path with nothing on it so this stays independent of whatever
/// the machine running the test happens to have at `~/.robco/env`.
#[test]
fn tui_launch_env_applies_session_env_even_without_autonomous_args() {
    let env_file = tempfile::tempdir().unwrap().path().join("env");
    let mut config = Config {
        default_program: "claude".into(),
        profiles: Vec::new(),
        ..Config::default()
    };
    config.overseer.session_env_file = Some(env_file);
    config
        .overseer
        .session_env
        .insert("ANTHROPIC_API_KEY".to_string(), "key".to_string());

    let (args, env) = tui_launch_env(&config);

    assert!(args.is_empty());
    assert_eq!(
        env,
        vec![("ANTHROPIC_API_KEY".to_string(), "key".to_string())]
    );
}
