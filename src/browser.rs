//! Opening a URL in the operator's default browser (dropr:499).
//!
//! macOS ships `open` with the base OS. Linux has no single equivalent, but
//! every major desktop environment ships `xdg-open` (part of `xdg-utils`),
//! which is the closest thing to a standard opener there. There is no one
//! command that exists on both platforms, so the command name is chosen per
//! target OS at compile time — robco's own CI runs both (`ubuntu-latest` and
//! `macos-latest`), so this file has to compile clean on each.
//!
//! The launcher process (`open` / `xdg-open`) hands the URL to a running
//! browser and exits fast; it does not wait for the browser itself to close.
//! `open` spawns it without waiting, so a slow or unusual `xdg-open` build
//! that *does* block until the browser exits cannot freeze the TUI. The
//! spawned child is reaped on a background thread instead of left as a
//! zombie for the rest of the session.

use std::{process::Command, thread};

#[cfg(target_os = "macos")]
const OPEN_COMMAND: &str = "open";

#[cfg(not(target_os = "macos"))]
const OPEN_COMMAND: &str = "xdg-open";

/// Launches the operator's default browser at `url`. Only reports whether the
/// launcher process itself started — once it has, this stops watching it, so
/// a browser that opens to an error page still counts as a successful launch
/// here. The realistic failure this catches is the launcher command missing
/// entirely (no `xdg-open` on a minimal Linux install, say).
pub fn open(url: &str) -> Result<(), String> {
    open_with(OPEN_COMMAND, url)
}

fn open_with(command: &str, url: &str) -> Result<(), String> {
    let mut child = Command::new(command)
        .arg(url)
        .spawn()
        .map_err(|err| format!("{command} failed to start: {err}"))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_missing_launcher_command() {
        let error = open_with(
            "robco-test-nonexistent-browser-launcher",
            "https://example.com",
        )
        .expect_err("a command that does not exist must not report success");
        assert!(error.contains("failed to start"));
    }
}
