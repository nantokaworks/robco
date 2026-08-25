use super::*;

#[test]
fn sanitizes_tmux_target_parts() {
    assert_eq!(sanitize_target_part("foo.bar:baz"), "foo-bar-baz");
    assert_eq!(
        session_name("robco_", "my.repo", "fix/thing"),
        "robco_my-repo_fix-thing"
    );
}

#[test]
fn exact_target_anchors_session_and_default_pane() {
    // `=` forces an exact session match (no prefix bleed into `<name>-shell`)
    // and the trailing `:` selects the default window/pane so the target
    // resolves for pane/window commands too (capture-pane, send-keys,
    // set-option window-size) — not just session-only commands.
    assert_eq!(exact("robco_repo_agent"), "=robco_repo_agent:");
}

#[test]
fn new_session_command_includes_environment_pairs() {
    let command = new_session_command_with_lookup(
        &TmuxServer::default_server(),
        "robco_repo_agent",
        Path::new("/repo"),
        "codex",
        &[("FIRST", "one".to_string()), ("SECOND", "two".to_string())],
        |_| None,
    );
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(args.windows(2).any(|args| args == ["-e", "FIRST=one"]));
    assert!(args.windows(2).any(|args| args == ["-e", "SECOND=two"]));
    assert_eq!(
        args.last().map(String::as_str),
        Some("unset NO_COLOR FORCE_COLOR COLORTERM CLICOLOR CLICOLOR_FORCE; codex")
    );
}

#[test]
fn new_session_command_neutralizes_missing_identity() {
    let command = new_session_command_with_lookup(
        &TmuxServer::default_server(),
        "robco_repo_shell",
        Path::new("/repo"),
        "zsh",
        &[],
        |_| None,
    );
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(
        args.windows(2)
            .any(|args| args == ["-e", "ROBCO_AGENT_ID="])
    );
    assert!(
        args.windows(2)
            .any(|args| args == ["-e", "ROBCO_PARENT_AGENT_ID="])
    );
}

#[test]
fn new_session_command_neutralizes_inherited_ai_identity() {
    let command = new_session_command_with_lookup(
        &TmuxServer::default_server(),
        "robco_repo_agent",
        Path::new("/repo"),
        "claude",
        &[],
        |_| None,
    );
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    for key in INHERITED_IDENTITY_KEYS {
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-e", &format!("{key}=")]),
            "expected {key} to be neutralized"
        );
    }
}

#[test]
fn new_session_command_keeps_caller_supplied_inherited_identity() {
    // A caller that deliberately hands a value for one of these keys (e.g. a
    // subagent that should keep its parent's Codex transcript path) is not
    // second-guessed — the same override rule `ENV_AGENT_ID` already gets.
    let command = new_session_command_with_lookup(
        &TmuxServer::default_server(),
        "robco_repo_agent",
        Path::new("/repo"),
        "claude",
        &[("CLAUDECODE", "1".to_string())],
        |_| None,
    );
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(args.windows(2).any(|pair| pair == ["-e", "CLAUDECODE=1"]));
    assert!(!args.contains(&"CLAUDECODE=".to_string()));
}
