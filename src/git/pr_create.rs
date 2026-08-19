use std::{path::Path, process::Command};

use super::{GIT_NETWORK_TIMEOUT, command_output};
use crate::{Result, exec::run_timeout};

/// Opens a pull request for `branch` with the given title and body, without
/// needing a worker session to run `gh pr create` itself. Returns the created
/// pull request's URL, which is what `gh pr create` prints to stdout on
/// success when `--title`/`--body` are given instead of prompting.
pub fn create_pr(repo: &Path, branch: &str, title: &str, body: &str) -> Result<String> {
    let mut command = Command::new("gh");
    command.current_dir(repo).args([
        "pr", "create", "--head", branch, "--title", title, "--body", body,
    ]);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    command_output(output, "gh pr create")
}
