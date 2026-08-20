use std::ffi::OsString;

use super::*;
use crate::overseer::config::ProtectionMode;

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[test]
fn legacy_overseer_run_rewrites_to_daemon() {
    let rewritten = rewrite_legacy_overseer(&os(&["robco", "overseer", "run"])).unwrap();
    assert_eq!(rewritten, os(&["robco", "daemon"]));
}

#[test]
fn legacy_overseer_install_service_rewrites_and_keeps_trailing_args() {
    let rewritten =
        rewrite_legacy_overseer(&os(&["robco", "overseer", "install-service"])).unwrap();
    assert_eq!(rewritten, os(&["robco", "service", "install"]));
}

#[test]
fn legacy_overseer_set_rewrites_to_config_set_and_keeps_the_arguments() {
    let rewritten =
        rewrite_legacy_overseer(&os(&["robco", "overseer", "set", "auto-merge", "on"])).unwrap();
    assert_eq!(
        rewritten,
        os(&["robco", "config", "set", "auto-merge", "on"])
    );
}

#[test]
fn legacy_overseer_status_debug_rewrites_and_keeps_the_flag() {
    let rewritten =
        rewrite_legacy_overseer(&os(&["robco", "overseer", "status", "--debug"])).unwrap();
    assert_eq!(rewritten, os(&["robco", "status", "--debug"]));
}

#[test]
fn unknown_legacy_overseer_subcommand_is_left_alone() {
    assert!(rewrite_legacy_overseer(&os(&["robco", "overseer", "bogus"])).is_none());
}

#[test]
fn bare_overseer_with_no_subcommand_is_left_alone() {
    assert!(rewrite_legacy_overseer(&os(&["robco", "overseer"])).is_none());
}

#[test]
fn overseer_appearing_after_the_subcommand_position_is_not_rewritten() {
    // A repo literally named "overseer" being renamed to "status" must not be
    // mistaken for the legacy `overseer status` command.
    assert!(rewrite_legacy_overseer(&os(&["robco", "rename", "overseer", "status"])).is_none());
}

#[test]
fn a_rewritten_legacy_invocation_still_parses_with_clap() {
    let raw = os(&["robco", "overseer", "protection", "relaxed"]);
    let rewritten = rewrite_legacy_overseer(&raw).unwrap();
    let args = Args::try_parse_from(&rewritten).unwrap();
    let Some(Command::Config(args)) = args.command else {
        panic!("expected config command")
    };
    let ConfigCommand::Protection(args) = args.command else {
        panic!("expected protection")
    };
    assert_eq!(args.mode, ProtectionMode::Relaxed);
}
