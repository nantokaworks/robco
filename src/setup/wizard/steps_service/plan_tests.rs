use std::{
    ffi::OsString,
    os::unix::process::ExitStatusExt,
    path::PathBuf,
    process::{ExitStatus, Output},
};

use super::{BootstrapMode, BootstrapPlan, SetenvPlan, service_is_absent};
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
                Ok(Output {
                    status: ExitStatus::from_raw(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            },
            || ServiceState::Loaded,
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
                OsString::from("bootstrap"),
                OsString::from("gui/501"),
                OsString::from("/tmp/robco.plist")
            ],
        ]
    );
}

#[test]
fn load_that_does_not_take_effect_is_an_error() {
    let error = bootstrap_plan(true, BootstrapMode::Load)
        .apply_with(
            &mut Vec::new(),
            |_| {
                Ok(Output {
                    status: ExitStatus::from_raw(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            },
            || ServiceState::Unloaded,
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
        .apply_with(
            &mut output,
            |_| {
                Ok(Output {
                    status: ExitStatus::from_raw(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            },
            || ServiceState::Loaded,
        )
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
