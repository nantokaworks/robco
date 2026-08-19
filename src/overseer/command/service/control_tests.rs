use std::{cell::Cell, time::Duration};

use super::*;

fn tiny_budget() -> SettleBudget {
    SettleBudget {
        polls: 3,
        poll_interval: Duration::from_millis(1),
        bootstrap_attempts: 1,
        retry_interval: Duration::from_millis(1),
    }
}

#[test]
fn wait_for_pid_exit_with_no_pidfile_is_already_gone() {
    assert!(wait_for_pid_exit_with(None, tiny_budget(), |_| {
        unreachable!("no pid to check liveness for")
    }));
}

#[test]
fn wait_for_pid_exit_with_a_prompt_exit_returns_on_the_first_check() {
    let calls = Cell::new(0);
    let gone = wait_for_pid_exit_with(Some(123), tiny_budget(), |_| {
        calls.set(calls.get() + 1);
        false
    });

    assert!(gone);
    assert_eq!(calls.get(), 1, "a pid already gone must not poll again");
}

#[test]
fn wait_for_pid_exit_with_a_slow_exit_that_lands_inside_the_budget() {
    let calls = Cell::new(0);
    let gone = wait_for_pid_exit_with(Some(123), tiny_budget(), |_| {
        let attempt = calls.get();
        calls.set(attempt + 1);
        // Alive for the first two checks, gone by the third — still inside
        // this budget's 3-poll ceiling.
        attempt < 2
    });

    assert!(gone);
    assert_eq!(calls.get(), 3);
}

#[test]
fn wait_for_pid_exit_with_a_process_that_never_exits_gives_up() {
    let calls = Cell::new(0);
    let gone = wait_for_pid_exit_with(Some(123), tiny_budget(), |_| {
        calls.set(calls.get() + 1);
        true
    });

    assert!(!gone);
    assert_eq!(calls.get(), tiny_budget().polls as usize);
}

#[test]
fn verify_new_process_with_succeeds_once_a_different_live_pid_shows_up() {
    let old_pid = Some(111);
    let calls = Cell::new(0);
    let result = verify_new_process_with(
        old_pid,
        tiny_budget(),
        || {
            let attempt = calls.get();
            calls.set(attempt + 1);
            // The pidfile still names the old pid for the first check, then
            // the new daemon's own pid shows up.
            if attempt == 0 { old_pid } else { Some(222) }
        },
        |pid| pid == 222,
    );

    assert!(result.is_ok());
}

#[test]
fn verify_new_process_with_fails_when_the_pidfile_never_changes() {
    let old_pid = Some(111);
    let result = verify_new_process_with(old_pid, tiny_budget(), || old_pid, |_| true);

    assert!(result.is_err());
}

#[test]
fn still_shutting_down_error_names_the_live_pid() {
    let error = still_shutting_down_error(Some(456));
    assert!(error.to_string().contains("456"));
}
