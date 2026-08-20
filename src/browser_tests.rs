use std::collections::HashMap;

use super::*;

/// Builds the `read` closure [`route`] takes, from a fixed set of variables.
/// Anything not listed reads as unset, which is what an operator's session
/// looks like for the variables it does not use.
fn session(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect();
    move |name: &str| vars.get(name).cloned()
}

#[test]
fn reports_a_missing_launcher_command() {
    let error = open_with(
        "robco-test-nonexistent-browser-launcher",
        "https://example.com",
    )
    .expect_err("a command that does not exist must not report success");
    assert!(error.contains("failed to start"));
}

#[test]
fn a_local_session_still_spawns_the_platform_launcher() {
    assert_eq!(
        route(session(&[])),
        Route::Launcher(OPEN_COMMAND.to_string())
    );
}

#[test]
fn each_ssh_variable_sends_the_url_to_the_terminal() {
    for name in SSH_SESSION_VARS {
        assert_eq!(
            route(session(&[(name, "10.0.0.2 51000 10.0.0.1 22")])),
            Route::Clipboard,
            "{name} must mark the session as remote"
        );
    }
}

#[test]
fn a_blank_ssh_variable_is_not_a_remote_session() {
    assert_eq!(
        route(session(&[("SSH_CONNECTION", "   ")])),
        Route::Launcher(OPEN_COMMAND.to_string())
    );
}

#[test]
fn browser_overrides_the_platform_launcher() {
    assert_eq!(
        route(session(&[("BROWSER", "my-opener")])),
        Route::Launcher("my-opener".to_string())
    );
}

#[test]
fn browser_wins_over_ssh_so_a_wired_up_opener_still_runs() {
    assert_eq!(
        route(session(&[
            ("BROWSER", " my-opener "),
            ("SSH_CONNECTION", "10.0.0.2 51000 10.0.0.1 22"),
        ])),
        Route::Launcher("my-opener".to_string())
    );
}

#[test]
fn a_blank_browser_falls_through_to_the_normal_choice() {
    assert_eq!(
        route(session(&[("BROWSER", "  ")])),
        Route::Launcher(OPEN_COMMAND.to_string())
    );
    assert_eq!(
        route(session(&[("BROWSER", ""), ("SSH_TTY", "/dev/pts/3"),])),
        Route::Clipboard
    );
}

#[test]
fn only_a_tmux_pane_reads_as_inside_tmux() {
    assert!(inside_tmux(session(&[(
        "TMUX",
        "/tmp/tmux-501/default,123,0"
    )])));
    assert!(!inside_tmux(session(&[])));
    assert!(!inside_tmux(session(&[("TMUX", "  ")])));
}

#[test]
fn osc52_wraps_the_base64_url_for_the_system_clipboard() {
    assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
}

#[test]
fn base64_pads_every_remainder() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foob"), "Zm9vYg==");
    assert_eq!(base64(b"fooba"), "Zm9vYmE=");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
}

#[test]
fn base64_covers_the_whole_alphabet() {
    // 0xfb 0xff spans the high end of the table, where an off-by-one in the
    // shift or the mask would show up.
    assert_eq!(base64(&[0xfb, 0xff, 0xfe]), "+//+");
    assert_eq!(
        base64("https://dropr.sh/nantokaworks/robco/tasks/nanoid-1".as_bytes()),
        "aHR0cHM6Ly9kcm9wci5zaC9uYW50b2thd29ya3Mvcm9iY28vdGFza3MvbmFub2lkLTE="
    );
}
