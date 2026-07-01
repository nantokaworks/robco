use std::{collections::BTreeMap, fs};

use serde::{Deserialize, Serialize};

use crate::{
    Result,
    config::{ensure_robco_dir, state_path},
    model::RepoNode,
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
        let mut known: BTreeMap<String, RepoNode> = self
            .repos
            .drain(..)
            .map(|repo| (repo.path.to_string_lossy().to_string(), repo))
            .collect();

        self.repos = discovered
            .into_iter()
            .map(|mut repo| {
                if let Some(existing) = known.remove(&repo.path.to_string_lossy().to_string()) {
                    // Carry over the tracked agents and runtime status so a
                    // re-scan does not drop worktrees or flicker the repo's
                    // main-session badge. Prefer a freshly-resolved dropr
                    // overlay, falling back to the previous one.
                    repo.agents = existing.agents;
                    repo.main_status = existing.main_status;
                    repo.main_last_capture = existing.main_last_capture;
                    repo.main_last_change_at = existing.main_last_change_at;
                    repo.dropr = repo.dropr.or(existing.dropr);
                }
                repo
            })
            .collect();
    }
}
