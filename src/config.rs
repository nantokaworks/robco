use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_program: String,
    pub branch_prefix: String,
    pub worktree_root: PathBuf,
    pub tmux_session_prefix: String,
    pub poll_interval_ms: u64,
    pub dropr_overlay: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_program: "claude".to_string(),
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

fn robco_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or(crate::Error::HomeDir)?;
    Ok(home.join(".robco"))
}

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}
