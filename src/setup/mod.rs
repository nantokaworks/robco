pub mod claude;
pub mod codex;
pub mod openclaw;

use std::{fs, io::ErrorKind, path::Path};

use crate::{
    Error, Result,
    cli::{InstallArgs, InstallTarget},
};

#[derive(Debug, Eq, PartialEq)]
pub enum Action {
    AlreadyPresent,
    FileCreated,
    Installed,
    NotPresent,
    Removed,
    Updated,
}

impl Action {
    fn as_str(&self) -> &'static str {
        match self {
            Self::AlreadyPresent => "already-present",
            Self::FileCreated => "file-created",
            Self::Installed => "installed",
            Self::NotPresent => "not-present",
            Self::Removed => "removed",
            Self::Updated => "updated",
        }
    }
}

pub fn install(args: &InstallArgs) -> Result<()> {
    for target in selected_targets(args) {
        let action = match target {
            InstallTarget::Claude => claude::install()?,
            InstallTarget::Codex => codex::install()?,
            InstallTarget::Openclaw => openclaw::install()?,
            InstallTarget::All => unreachable!("all is expanded before installation"),
        };
        println!("{}: {}", target_name(target), action.as_str());
    }
    Ok(())
}

pub fn uninstall(args: &InstallArgs) -> Result<()> {
    for target in selected_targets(args) {
        let action = match target {
            InstallTarget::Claude => claude::uninstall()?,
            InstallTarget::Codex => codex::uninstall()?,
            InstallTarget::Openclaw => openclaw::uninstall()?,
            InstallTarget::All => unreachable!("all is expanded before uninstallation"),
        };
        println!("{}: {}", target_name(target), action.as_str());
    }
    Ok(())
}

fn selected_targets(args: &InstallArgs) -> Vec<InstallTarget> {
    if args.all || args.target == InstallTarget::All {
        vec![
            InstallTarget::Claude,
            InstallTarget::Codex,
            InstallTarget::Openclaw,
        ]
    } else {
        vec![args.target]
    }
}

fn target_name(target: InstallTarget) -> &'static str {
    match target {
        InstallTarget::Claude => "claude",
        InstallTarget::Codex => "codex",
        InstallTarget::Openclaw => "openclaw",
        InstallTarget::All => "all",
    }
}

fn home_dir() -> Result<std::path::PathBuf> {
    dirs::home_dir().ok_or(Error::HomeDir)
}

fn read_optional_config(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests;
