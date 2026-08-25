use nanoid::nanoid;

use super::*;
use crate::tmux::is_installed;

/// `true` (and prints a skip notice) when there is no real `tmux` to drive —
/// GitHub's hosted `macos-latest` runner ships none, unlike `ubuntu-latest`.
fn skip_without_tmux() -> bool {
    if !is_installed() {
        eprintln!("skipping: no tmux binary on this runner");
        return true;
    }
    false
}

fn test_session_name(label: &str) -> String {
    format!("robco-test-launch-{label}-{}", nanoid!(6))
}

#[test]
fn new_worker_session_reports_the_crashed_programs_own_output() {
    if skip_without_tmux() {
        return;
    }
    let session = test_session_name("crash");
    let cwd = std::env::temp_dir();

    let result = new_worker_session(&session, &cwd, "sh -c 'echo boom-detail; exit 7'", &[]);
    let _ = kill_session(&session);

    match result {
        Err(Error::WorkerLaunchCrashed { detail, .. }) => {
            assert!(detail.contains("boom-detail"), "detail was {detail:?}");
        }
        other => panic!("expected WorkerLaunchCrashed, got {other:?}"),
    }
}

#[test]
fn new_worker_session_accepts_a_pane_that_stays_alive_in_the_right_directory() {
    if skip_without_tmux() {
        return;
    }
    let session = test_session_name("ok");
    let cwd = std::env::temp_dir();

    let result = new_worker_session(&session, &cwd, "sleep 5", &[]);
    let _ = kill_session(&session);

    assert!(result.is_ok(), "expected launch to verify, got {result:?}");
}

#[test]
fn verify_launch_refuses_a_pane_started_in_the_wrong_directory() {
    if skip_without_tmux() {
        return;
    }
    let session = test_session_name("wrong-cwd");
    let actual_cwd = std::env::temp_dir();
    let output = session::new_session_command(&session, &actual_cwd, "sleep 5", &[])
        .output()
        .unwrap();
    assert!(output.status.success());

    // A directory that plainly disagrees with what the session was actually
    // given, mimicking dropr:554's real symptom: the pane started somewhere
    // other than the `-c` argument it was launched with.
    let expected_cwd = actual_cwd.join("not-where-the-pane-landed");
    let log_path = output_tap_path(&session);
    let result = verify_launch(&session, &expected_cwd, &log_path);
    let _ = kill_session(&session);

    match result {
        Err(Error::WorkerLaunchWrongCwd { expected, .. }) => {
            assert_eq!(expected, expected_cwd);
        }
        other => panic!("expected WorkerLaunchWrongCwd, got {other:?}"),
    }
}
