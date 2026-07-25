use std::{
    ffi::OsString,
    os::unix::process::ExitStatusExt,
    path::PathBuf,
    process::{ExitStatus, Output},
    time::Duration,
};

use super::{BootstrapMode, BootstrapPlan, SetenvPlan, SettleBudget, service_is_absent};
use crate::setup::wizard::steps_service::{ServicePlan, probe::ServiceState};

fn bootstrap_plan(execute: bool, mode: BootstrapMode) -> BootstrapPlan {
    BootstrapPlan {
        domain: "gui/501".into(),
        path: PathBuf::from("/tmp/robco.plist"),
        executable: PathBuf::from("/opt/homebrew/bin/robco"),
        execute,
        mode,
    }
}

/// Zero intervals keep the injected-runner tests instant; only the attempt caps
/// matter for the behaviour under test.
fn budget() -> SettleBudget {
    SettleBudget {
        polls: 4,
        poll_interval: Duration::ZERO,
        bootstrap_attempts: 3,
        retry_interval: Duration::ZERO,
    }
}

fn launchctl(code: i32, stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(code << 8),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

fn ok() -> Output {
    launchctl(0, "")
}

#[test]
fn copyable_setenv_command_precedes_bootstrap() {
    let plan = ServicePlan {
        setenv: Some(SetenvPlan {
            name: "DISCORD_TOKEN".into(),
            value: None,
        }),
        bootstrap: bootstrap_plan(false, BootstrapMode::Load),
    };
    let mut output = Vec::new();
    plan.apply(&mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.find("launchctl setenv").unwrap() < output.find("launchctl bootstrap").unwrap());
}

#[test]
fn manual_setenv_defers_accepted_reload() {
    let plan = ServicePlan {
        setenv: Some(SetenvPlan {
            name: "DISCORD_TOKEN".into(),
            value: None,
        }),
        bootstrap: bootstrap_plan(true, BootstrapMode::Reload),
    };
    let mut output = Vec::new();
    plan.apply(&mut output).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Automatic service loading or reloading deferred"));
    assert!(output.contains("launchctl setenv"));
    assert!(!output.contains("launchctl bootout"));
    assert!(!output.contains("launchctl bootstrap"));
}

#[test]
fn reload_renders_bootout_before_bootstrap() {
    let mut output = Vec::new();
    bootstrap_plan(false, BootstrapMode::Reload)
        .apply(&mut output)
        .unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(
        output.find("launchctl bootout").unwrap() < output.find("launchctl bootstrap").unwrap()
    );
}

#[test]
fn reload_executes_bootout_before_bootstrap() {
    let mut invocations = Vec::new();
    bootstrap_plan(true, BootstrapMode::Reload)
        .apply_with(
            &mut Vec::new(),
            |args| {
                invocations.push(args.to_vec());
                // The teardown has already landed, so the first poll clears.
                Ok(if args[0] == "print" {
                    launchctl(113, "Could not find service")
                } else {
                    ok()
                })
            },
            || ServiceState::Loaded,
            budget(),
        )
        .unwrap();

    assert_eq!(
        invocations,
        vec![
            vec![
                OsString::from("bootout"),
                OsString::from("gui/501/com.robco.overseer")
            ],
            vec![
                OsString::from("print"),
                OsString::from("gui/501/com.robco.overseer")
            ],
            vec![
                OsString::from("bootstrap"),
                OsString::from("gui/501"),
                OsString::from("/tmp/robco.plist")
            ],
        ]
    );
}

#[test]
fn reload_waits_for_a_settling_bootout_before_bootstrapping() {
    let mut polls = 0;
    let mut bootstrapped_after = None;
    bootstrap_plan(true, BootstrapMode::Reload)
        .apply_with(
            &mut Vec::new(),
            |args| {
                if args[0] == "print" {
                    polls += 1;
                    // The daemon is still on its way out for the first two polls.
                    return Ok(if polls < 3 {
                        ok()
                    } else {
                        launchctl(113, "Could not find service")
                    });
                }
                if args[0] == "bootstrap" {
                    bootstrapped_after = Some(polls);
                }
                Ok(ok())
            },
            || ServiceState::Loaded,
            budget(),
        )
        .unwrap();

    assert_eq!(polls, 3);
    assert_eq!(bootstrapped_after, Some(3));
}

#[test]
fn a_bootout_that_never_lands_reports_the_manual_recovery() {
    let mut polls = 0;
    let error = bootstrap_plan(true, BootstrapMode::Reload)
        .apply_with(
            &mut Vec::new(),
            |args| {
                if args[0] == "print" {
                    polls += 1;
                }
                assert_ne!(args[0], "bootstrap");
                Ok(ok())
            },
            || ServiceState::Loaded,
            budget(),
        )
        .unwrap_err()
        .to_string();

    assert_eq!(polls, budget().polls);
    assert!(error.contains("still shutting down"));
    assert!(error.contains("launchctl bootstrap gui/501 /tmp/robco.plist"));
}

#[test]
fn a_bootstrap_onto_a_busy_domain_is_retried() {
    let mut attempts = 0;
    bootstrap_plan(true, BootstrapMode::Load)
        .apply_with(
            &mut Vec::new(),
            |args| {
                assert_eq!(args[0], "bootstrap");
                attempts += 1;
                Ok(if attempts == 1 {
                    launchctl(5, "Bootstrap failed: 5: Input/output error")
                } else {
                    ok()
                })
            },
            || ServiceState::Loaded,
            budget(),
        )
        .unwrap();

    assert_eq!(attempts, 2);
}

#[test]
fn a_domain_that_stays_busy_reports_the_manual_recovery() {
    let mut attempts = 0;
    let error = bootstrap_plan(true, BootstrapMode::Load)
        .apply_with(
            &mut Vec::new(),
            |_| {
                attempts += 1;
                Ok(launchctl(5, "Bootstrap failed: 5: Input/output error"))
            },
            || ServiceState::Loaded,
            budget(),
        )
        .unwrap_err()
        .to_string();

    assert_eq!(attempts, budget().bootstrap_attempts);
    assert!(error.contains("Input/output error"));
    assert!(error.contains("still shutting down"));
    assert!(error.contains("launchctl bootstrap gui/501 /tmp/robco.plist"));
}

#[test]
fn a_bootstrap_failure_that_is_not_transient_is_not_retried() {
    let mut attempts = 0;
    let error = bootstrap_plan(true, BootstrapMode::Load)
        .apply_with(
            &mut Vec::new(),
            |_| {
                attempts += 1;
                Ok(launchctl(
                    112,
                    "Bootstrap failed: 112: Operation not permitted",
                ))
            },
            || ServiceState::Loaded,
            budget(),
        )
        .unwrap_err()
        .to_string();

    assert_eq!(attempts, 1);
    assert!(error.contains("Operation not permitted"));
    assert!(!error.contains("still shutting down"));
}

#[test]
fn load_that_does_not_take_effect_is_an_error() {
    let error = bootstrap_plan(true, BootstrapMode::Load)
        .apply_with(
            &mut Vec::new(),
            |_| Ok(ok()),
            || ServiceState::Unloaded,
            budget(),
        )
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("did not come up after bootstrap")
    );
}

#[test]
fn successful_load_reports_the_loaded_state() {
    let mut output = Vec::new();
    bootstrap_plan(true, BootstrapMode::Load)
        .apply_with(&mut output, |_| Ok(ok()), || ServiceState::Loaded, budget())
        .unwrap();

    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("launchd ··········· loaded")
    );
}

#[test]
fn only_absent_service_bootout_error_is_tolerated() {
    assert!(service_is_absent(
        Some(3),
        b"Boot-out failed: 3: No such process"
    ));
    assert!(!service_is_absent(Some(3), b"Permission denied"));
    assert!(!service_is_absent(Some(5), b"Input/output error"));
}
