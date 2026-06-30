use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_program: String,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    pub branch_prefix: String,
    pub worktree_root: PathBuf,
    pub tmux_session_prefix: String,
    pub poll_interval_ms: u64,
    pub dropr_overlay: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub program: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_program: "claude".to_string(),
            profiles: Vec::new(),
            branch_prefix: "robco/".to_string(),
            worktree_root: home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".robco")
                .join("worktrees"),
            tmux_session_prefix: "robco_".to_string(),
            poll_interval_ms: 750,
            dropr_overlay: true,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(path)?;
        let mut config: Config = serde_json::from_str(&raw)?;
        if config.default_program.trim().is_empty() {
            config.default_program = "claude".to_string();
        }
        Ok(config)
    }

    pub fn default_program_command(&self) -> String {
        self.profiles
            .iter()
            .find(|profile| profile.name == self.default_program)
            .map(|profile| profile.program.clone())
            .unwrap_or_else(|| self.default_program.clone())
    }
}

pub fn state_path() -> Result<PathBuf> {
    Ok(robco_dir()?.join("state.json"))
}

pub fn ensure_robco_dir() -> Result<PathBuf> {
    let dir = robco_dir()?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn config_path() -> Result<PathBuf> {
    Ok(robco_dir()?.join("config.json"))
}

pub fn config_file_path() -> Result<PathBuf> {
    config_path()
}

fn robco_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or(crate::Error::HomeDir)?;
    Ok(home.join(".robco"))
}

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_default_program_through_profiles() {
        let config = Config {
            default_program: "codex".to_string(),
            profiles: vec![Profile {
                name: "codex".to_string(),
                program: "codex --ask-for-approval never".to_string(),
            }],
            ..Config::default()
        };

        assert_eq!(
            config.default_program_command(),
            "codex --ask-for-approval never"
        );
    }
}
