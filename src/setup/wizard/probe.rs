use std::{io::Write, process::Command, time::Duration};

use crate::chief::exec::run_timeout;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProbeResult {
    pub name: &'static str,
    pub required: bool,
    pub ok: bool,
}

pub(crate) fn run() -> Vec<ProbeResult> {
    run_with(|name, version_flag| {
        let mut command = Command::new(name);
        command.arg(version_flag);
        run_timeout(command, Duration::from_secs(5)).is_ok_and(|output| output.status.success())
    })
}

pub(crate) fn run_with(mut check: impl FnMut(&str, &str) -> bool) -> Vec<ProbeResult> {
    [
        ("git", true, "--version"),
        // tmux rejects the long `--version` flag; it only accepts `-V`.
        ("tmux", true, "-V"),
        ("gh", false, "--version"),
        ("dropr", false, "--version"),
    ]
    .into_iter()
    .map(|(name, required, version_flag)| ProbeResult {
        name,
        required,
        ok: check(name, version_flag),
    })
    .collect()
}

pub(crate) fn render<W: Write>(output: &mut W, results: &[ProbeResult]) -> std::io::Result<()> {
    writeln!(output, "▌ robco ▸ prerequisite scan")?;
    for result in results {
        let status = if result.ok { "OK" } else { "NG" };
        let note = if !result.ok && !result.required {
            " (warning)"
        } else {
            ""
        };
        writeln!(output, "  {} ··············· {status}{note}", result.name)?;
    }
    Ok(())
}

pub(crate) fn missing_required(results: &[ProbeResult]) -> bool {
    results.iter().any(|result| result.required && !result.ok)
}
