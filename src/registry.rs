use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

use fd_lock::RwLock;
use nanoid::nanoid;

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

    /// Read the stored registry under the cross-process lock.
    ///
    /// [`Registry::load`] already sees a whole file — a writer renames a
    /// finished temp file into place — but a reader that then acts on what it
    /// read wants the value a [`Registry::locked_update`] transaction settled
    /// on, not one it is midway through replacing. The shared lock waits that
    /// transaction out; it never blocks another reader.
    pub fn locked_load() -> Result<Self> {
        Self::locked_load_at(&state_path()?)
    }

    fn locked_load_at(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                version: 1,
                repos: Vec::new(),
            });
        }
        Self::with_read_lock(path, || {
            let raw = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&raw)?)
        })
    }

    pub fn save(&self) -> Result<()> {
        ensure_robco_dir()?;
        let path = state_path()?;
        self.save_at(&path)
    }

    pub fn add_pinned(&mut self, path: &Path) -> Result<bool> {
        let path = path.canonicalize()?;
        Ok(self.add_canonical_pinned(path))
    }

    pub fn locked_add_pinned(path: &Path) -> Result<()> {
        let path = path.canonicalize()?;
        Self::locked_update(|registry| {
            registry.add_canonical_pinned(path);
        })
        .map(|_| ())
    }

    /// Pin an already-canonicalized path, reporting whether anything changed.
    /// Callers holding the registry lock use this directly so the mutation
    /// closure stays infallible.
    pub(crate) fn add_canonical_pinned(&mut self, path: PathBuf) -> bool {
        if let Some(repo) = self.repos.iter_mut().find(|repo| repo.path == path) {
            let changed = !repo.pinned;
            repo.pinned = true;
            return changed;
        }
        self.repos.push(crate::discover::repo_node(path, true));
        true
    }

    /// Serialize a registry read-modify-write transaction across processes.
    pub fn locked_update<F>(f: F) -> Result<Registry>
    where
        F: FnOnce(&mut Registry),
    {
        ensure_robco_dir()?;
        Self::locked_update_at(&state_path()?, f)
    }

    fn locked_update_at<F>(path: &Path, f: F) -> Result<Registry>
    where
        F: FnOnce(&mut Registry),
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Self::with_write_lock(path, || {
            let mut registry = if path.exists() {
                let raw = fs::read_to_string(path)?;
                serde_json::from_str(&raw)?
            } else {
                Registry {
                    version: 1,
                    repos: Vec::new(),
                }
            };
            f(&mut registry);
            Self::write_unlocked(&registry, path)?;
            Ok(registry)
        })
    }

    fn save_at(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Self::with_write_lock(path, || Self::write_unlocked(self, path))
    }

    fn with_write_lock<T>(path: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let mut lock = RwLock::new(Self::open_lock_file(path)?);
        let _guard = lock.write()?;
        f()
    }

    fn with_read_lock<T>(path: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let lock = RwLock::new(Self::open_lock_file(path)?);
        let _guard = lock.read()?;
        f()
    }

    fn open_lock_file(path: &Path) -> Result<std::fs::File> {
        Ok(OpenOptions::new()
            .create(true)
            // Lock file contents are irrelevant; keep them untouched.
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("json.lock"))?)
    }

    fn write_unlocked(registry: &Registry, path: &Path) -> Result<()> {
        let raw = serde_json::to_string_pretty(registry)?;
        let temp_path = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let written = fs::write(&temp_path, raw).and_then(|()| fs::rename(&temp_path, path));
        if let Err(error) = written {
            let _ = fs::remove_file(temp_path);
            return Err(error.into());
        }
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
                    repo.pinned = repo.pinned || existing.pinned;
                    repo.agents = existing.agents;
                    repo.main_status = existing.main_status;
                    repo.main_last_capture = existing.main_last_capture;
                    repo.main_last_change_at = existing.main_last_change_at;
                    repo.dropr = repo.dropr.or(existing.dropr);
                }
                repo
            })
            .collect();

        // Repos registered from another launch directory are never in the
        // discovered set. Keep ones that still track agents, plus pinned manual
        // registrations. Agent-less, unpinned leftovers are dropped.
        self.repos.extend(
            known
                .into_values()
                .filter(|repo| !repo.agents.is_empty() || repo.pinned),
        );
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
