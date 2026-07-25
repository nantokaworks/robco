use super::*;
use crate::overseer::autonomy::AutonomyLevel;

#[test]
fn toggle_line_reports_no_switch_the_daemon_ignores() {
    let config = OverseerConfig::default();
    assert!(config.dispatch_enabled);
    let line = toggle_line(&config, false);
    assert_eq!(
        line,
        "dispatch: on  auto-merge: off (protection: required)  autonomy: conservative  circuit: closed"
    );
    // A dispatching daemon must never be described as switched off.
    assert!(!line.contains("overseer: off"));
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
            toggle_line(&config, false).contains(&format!("autonomy: {label}")),
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
    assert!(toggle_line(&config, true).starts_with("dispatch: off"));
}
