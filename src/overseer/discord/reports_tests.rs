use super::*;

#[test]
fn status_line_reports_no_switch_the_daemon_ignores() {
    let config = OverseerConfig::default();
    assert!(config.dispatch_enabled);
    let line = status_line(&config, 1, 4);
    assert_eq!(
        line,
        "**dispatch** on\n**automerge** off\n**autonomy** conservative\n**workers** 1\n**today** 4/20"
    );
    // A dispatching daemon must never be described as switched off.
    assert!(!line.contains("overseer=off"));
}
