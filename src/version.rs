use std::process::Command;

pub fn render() -> String {
    format!(
        "▌ robco ▸ version\n  █▀▄ █▀█ █▀▄ █▀▀ █▀█\n  █▀▄ █▄█ █▄▀ █▄▄ █▄█\n  version ················· {}\n",
        env!("CARGO_PKG_VERSION")
    )
}

pub fn print() {
    print!("{}", render());
}

/// The version compiled into this running process — the version whose
/// compiled-in hook templates `agent::hooks::write_report_hooks` writes.
pub const RUNNING: &str = env!("CARGO_PKG_VERSION");

/// Asks whatever `robco` binary the `PATH` resolves to right now for its
/// version. That can be newer than [`RUNNING`] when this process is a
/// long-lived MCP server started before a `brew upgrade` replaced the
/// binary on disk (dropr:559): the OS-level `robco` moved on, this
/// process's compiled-in template did not. `None` when it cannot be
/// determined at all — no `robco` on `PATH`, or output that does not
/// parse — which a caller must treat as "nothing to compare against", not
/// as a mismatch.
pub fn installed() -> Option<String> {
    let output = Command::new("robco").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .split_whitespace()
        .last()
        .map(str::to_string)
}

/// Parses a `major.minor.patch` version string into numeric parts —
/// lexicographic string comparison would rank `"0.6.10"` below `"0.6.9"`.
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().split('.');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ))
}

/// `true` only when `installed` parses as strictly newer than `running`.
/// Either string failing to parse degrades to `false`: a spawn should never
/// be second-guessed over a version string this function cannot understand.
pub fn is_outdated(running: &str, installed: &str) -> bool {
    match (parse(running), parse(installed)) {
        (Some(running), Some(installed)) => running < installed,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_contains_crate_version() {
        assert!(render().contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn render_contains_non_empty_logo_lines() {
        let output = render();
        let mut lines = output.lines();
        assert_eq!(lines.next(), Some("▌ robco ▸ version"));
        assert!(lines.next().is_some_and(|line| !line.trim().is_empty()));
        assert!(lines.next().is_some_and(|line| !line.trim().is_empty()));
    }

    #[test]
    fn newer_installed_version_is_outdated() {
        assert!(is_outdated("0.6.2", "0.6.3"));
    }

    #[test]
    fn same_version_is_not_outdated() {
        assert!(!is_outdated("0.6.3", "0.6.3"));
    }

    #[test]
    fn older_installed_version_is_not_outdated() {
        assert!(!is_outdated("0.6.3", "0.6.2"));
    }

    /// Numeric, not lexicographic: `"0.6.9"` sorts after `"0.6.10"` as
    /// strings but must not read as outdated.
    #[test]
    fn double_digit_patch_compares_numerically() {
        assert!(!is_outdated("0.6.10", "0.6.9"));
        assert!(is_outdated("0.6.9", "0.6.10"));
    }

    #[test]
    fn unparseable_version_is_never_outdated() {
        assert!(!is_outdated("0.6.3", "not-a-version"));
        assert!(!is_outdated("not-a-version", "0.6.3"));
    }
}
