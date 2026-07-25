use super::*;
use crate::overseer::autonomy::AutonomyLevel;

#[test]
fn toggle_line_reports_no_switch_the_daemon_ignores() {
    let config = OverseerConfig::default();
    assert!(config.dispatch_enabled);
    let line = toggle_line(&config, false, 0);
    assert_eq!(
        line,
        "dispatch: on  auto-merge: off (protection: required)  autonomy: conservative  merge-recovery: off  circuit: closed"
    );
    // A dispatching daemon must never be described as switched off.
    assert!(!line.contains("overseer: off"));
}

#[test]
fn toggle_line_reports_the_merge_recovery_budget_with_its_switch() {
    // A worker handback is a worker turn, so an operator reading the status line
    // has to see how many are left before a stuck pull request reaches them.
    let config = OverseerConfig {
        merge_recovery_enabled: true,
        max_merge_recoveries: 3,
        ..OverseerConfig::default()
    };
    assert!(toggle_line(&config, false, 0).contains("merge-recovery: on (max 3)"));
}

#[test]
fn toggle_line_reports_what_a_switched_off_recovery_has_dropped() {
    // `off` on its own is a flag. What an operator needs in order to decide
    // whether to switch it on is what the flag has cost — failures a worker could
    // have fixed that reached nobody.
    let config = OverseerConfig::default();
    assert!(!config.merge_recovery_enabled);
    assert!(toggle_line(&config, false, 4).contains("merge-recovery: off (4 dropped)"));
    // Nothing dropped is not an exception worth a number.
    assert!(toggle_line(&config, false, 0).contains("merge-recovery: off  "));
}

#[test]
fn toggle_line_reports_the_autonomy_level_the_envelope_runs_under() {
    // The level decides how much the merge envelope clears on its own, so a
    // status line that omits it leaves a widened envelope indistinguishable from
    // the default one.
    for (level, label) in [
        (AutonomyLevel::ApprovalOnly, "approval_only"),
        (AutonomyLevel::Conservative, "conservative"),
        (AutonomyLevel::FullAuto, "full_auto"),
    ] {
        let config = OverseerConfig {
            autonomy_level: level,
            ..OverseerConfig::default()
        };
        assert!(
            toggle_line(&config, false, 0).contains(&format!("autonomy: {label}")),
            "expected autonomy: {label}"
        );
    }
}

#[test]
fn toggle_line_reports_dispatch_off_when_dispatch_is_disabled() {
    let config = OverseerConfig {
        dispatch_enabled: false,
        ..OverseerConfig::default()
    };
    assert!(toggle_line(&config, true, 0).starts_with("dispatch: off"));
}

#[test]
fn the_review_line_separates_the_pass_from_its_reviewer_model() {
    // The findings pass runs on its interval either way, so a missing profile is
    // "no model read the digest", not "nothing looked". Printing one word for
    // both is what let an inert review pass read as a quiet board.
    let mut config = OverseerConfig::default();
    assert!(config.review_profile.is_none());
    let without = review_state(&config);
    assert!(without.contains("findings every 20m"));
    assert!(without.contains("no reviewer model"));
    assert!(!without.contains("disabled"));

    config.review_profile = Some("claude".into());
    assert_eq!(review_state(&config), "every 20m via claude");
}

#[test]
fn daemon_line_names_the_build_the_daemon_started_from() {
    // The whole point of the field: `healthy` says the daemon is up, never that
    // it carries what has been merged since it started.
    let line = daemon_line(
        true,
        Some(1234),
        Some(Duration::from_secs(4)),
        Some("0.1.66"),
    );
    assert_eq!(line, "daemon: healthy pid=1234 heartbeat=4s version=0.1.66");
}

#[test]
fn daemon_line_survives_a_heartbeat_from_before_version_recording() {
    // An older daemon leaves the field out entirely; the line must still render
    // rather than fail the status command.
    let line = daemon_line(false, None, None, None);
    assert_eq!(
        line,
        "daemon: down/stale pid=- heartbeat=missing version=unknown"
    );
}
