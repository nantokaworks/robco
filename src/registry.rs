use std::{collections::BTreeMap, fs};

use serde::{Deserialize, Serialize};

use crate::{
    Result,
    config::{ensure_robco_dir, state_path},
    model::{AgentNode, RepoNode},
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    pub repos: Vec<RepoNode>,
}

impl Registry {
    pub fn load() -> Result<Self> {
        let path = state_path()?;
        if !path.exists() {
            return Ok(Self {
                version: 1,
                repos: Vec::new(),
            });
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save(&self) -> Result<()> {
        ensure_robco_dir()?;
        let path = state_path()?;
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(path, raw)?;
        Ok(())
    }

    pub fn merge_discovered(&mut self, discovered: Vec<RepoNode>) {
        let mut known_agents: BTreeMap<String, Vec<AgentNode>> = self
            .repos
            .drain(..)
            .map(|repo| (repo.path.to_string_lossy().to_string(), repo.agents))
            .collect();

        self.repos = discovered
            .into_iter()
            .map(|mut repo| {
                repo.agents = known_agents
                    .remove(&repo.path.to_string_lossy().to_string())
                    .unwrap_or_default();
                repo
            })
            .collect();
    }
}
