//! The discovery worker's off-thread capture pass.
//!
//! Runs on a clone of the registry and returns everything the UI thread needs
//! to apply the result; see `background_refresh` for the scheduling and ingest
//! side.

use std::{collections::HashSet, path::PathBuf, time::SystemTime};

use crate::{
    config::Config, discover, git, model::OrphanSession, registry::Registry,
    subagents::claude::ClaudeSubagentReader,
};

use super::{
    background_support::fingerprint,
    children, discovery,
    dropr_overlay::{self, OverlayStatus},
    orphans, subagents,
};

pub(super) struct DiscoveryResult {
    pub(super) registry: Registry,
    pub(super) fingerprint: Vec<u8>,
    pub(super) orphans: Option<Vec<OrphanSession>>,
    pub(super) save: bool,
    /// Outcome of this cycle's overlay load, or `None` when it did not load one.
    pub(super) overlay: Option<OverlayStatus>,
}

pub(super) fn capture_discovery(
    mut registry: Registry,
    config: Config,
    roots: Vec<PathBuf>,
    reload_overlay: bool,
) -> DiscoveryResult {
    let fingerprint = fingerprint(&registry);
    if !config.subagent_indicator {
        subagents::ingest(
            &mut registry.repos,
            false,
            &ClaudeSubagentReader::default(),
            SystemTime::now(),
        );
    }
    let discovered = discover::discover_all(roots.iter().map(PathBuf::as_path));
    let removed = discovery::prune_unmanaged(&mut registry.repos, &config.worktree_root);
    let expected = discovered
        .iter()
        .map(|repo| discovery::path_key(&repo.path))
        .chain(
            registry
                .repos
                .iter()
                .filter(|repo| !repo.agents.is_empty() || repo.pinned)
                .map(|repo| discovery::path_key(&repo.path)),
        )
        .collect::<HashSet<_>>();
    let current = registry
        .repos
        .iter()
        .map(|repo| discovery::path_key(&repo.path))
        .collect::<HashSet<_>>();
    let repos_changed = expected != current;
    if repos_changed {
        registry.merge_discovered(discovered);
    }
    // Resolve links for every repository, not just the discovered ones: a
    // pinned repo outside the scanned roots is never in the discovered set.
    let overlay = (config.dropr_overlay && (reload_overlay || repos_changed))
        .then(|| dropr_overlay::load_and_apply(&mut registry.repos));
    let mut added = false;
    for repo in &mut registry.repos {
        if let Ok(worktrees) = git::list_worktrees(&repo.path) {
            added |= children::reconcile(repo, &config, worktrees).0;
        }
    }
    if config.subagent_indicator {
        subagents::ingest(
            &mut registry.repos,
            true,
            &ClaudeSubagentReader::default(),
            SystemTime::now(),
        );
    }
    let found_orphans = orphans::discover_orphans(
        &registry.repos,
        &config.tmux_session_prefix,
        &config.worktree_root,
    );
    DiscoveryResult {
        registry,
        fingerprint,
        orphans: found_orphans,
        save: repos_changed || added || removed,
        overlay,
    }
}
