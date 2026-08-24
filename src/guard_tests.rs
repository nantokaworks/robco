use super::*;

/// The command that actually happened (dropr:552): a worker meant to isolate
/// its probe with `TMUX_TMPDIR` and ended the operator's own chat instead.
#[test]
fn blocks_the_command_that_ended_every_session() {
    assert!(ends_a_shared_tmux_server(
        "TMUX_TMPDIR=\"$D\" tmux kill-server 2>/dev/null"
    ));
}

#[test]
fn blocks_a_server_kill_hidden_inside_a_longer_line() {
    assert!(ends_a_shared_tmux_server(
        "out=$(BIN --test-threads 8 tmux 2>&1; TMUX_TMPDIR=\"$D\" tmux kill-server 2>/dev/null)"
    ));
}

#[test]
fn blocks_a_server_kill_by_absolute_path() {
    assert!(ends_a_shared_tmux_server(
        "/opt/homebrew/bin/tmux kill-server"
    ));
}

#[test]
fn blocks_keeping_only_the_current_session() {
    assert!(ends_a_shared_tmux_server("tmux kill-session -a"));
}

#[test]
fn blocks_reaching_the_server_by_process_name() {
    assert!(ends_a_shared_tmux_server("pkill -f tmux"));
    assert!(ends_a_shared_tmux_server("killall tmux"));
}

/// The escape hatch the deny message points at. A call that names its own
/// server cannot reach the shared one, so an isolated probe stays possible —
/// including ending that probe's server.
#[test]
fn allows_a_call_that_names_its_own_server() {
    assert!(!ends_a_shared_tmux_server(
        "env -u TMUX tmux -S /tmp/probe.sock kill-server"
    ));
    assert!(!ends_a_shared_tmux_server("tmux -L probe kill-server"));
    assert!(!ends_a_shared_tmux_server(
        "tmux -S /tmp/probe.sock kill-session -a"
    ));
}

/// robco kills single sessions itself, on merge and on request. Blocking that
/// would break the tool this guard exists to protect.
#[test]
fn allows_closing_one_named_session() {
    assert!(!ends_a_shared_tmux_server(
        "tmux kill-session -t '=robco_app_x'"
    ));
}

#[test]
fn allows_ordinary_tmux_and_unrelated_commands() {
    for command in [
        "tmux ls",
        "tmux new-session -d -s probe",
        "cargo test tmux -- --test-threads 8",
        "git log --oneline",
        "pkill -f node",
    ] {
        assert!(!ends_a_shared_tmux_server(command), "{command}");
    }
}

/// A session whose name begins with a dash must not read as a `-a` flag.
#[test]
fn a_dashed_session_name_is_not_a_flag() {
    assert!(!ends_a_shared_tmux_server(
        "tmux kill-session -t --all-hands"
    ));
}

#[test]
fn the_deny_payload_carries_the_pretooluse_decision() {
    let payload: serde_json::Value = serde_json::from_str(&deny("because")).unwrap();
    assert_eq!(payload["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(payload["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        payload["hookSpecificOutput"]["permissionDecisionReason"],
        "because"
    );
}
