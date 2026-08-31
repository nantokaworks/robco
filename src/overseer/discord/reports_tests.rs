use super::*;

#[test]
fn status_line_reports_no_switch_the_daemon_ignores() {
    let config = OverseerConfig::default();
    let line = status_line(&config, 1);
    assert_eq!(line, "**merge-gate** off\n**workers** 1");
}
