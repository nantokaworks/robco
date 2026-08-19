use std::{path::Path, process::Command};

use super::{GIT_LOCAL_TIMEOUT, command_output};
use crate::{Result, exec::run_timeout};

pub(crate) fn local_branch_commit(repo: &Path, branch: &str) -> Result<String> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["rev-parse"])
        .arg(format!("refs/heads/{branch}"));
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    command_output(output, "git rev-parse local branch")
}
