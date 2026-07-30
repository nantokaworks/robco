//! The discovery worker's off-thread capture pass.
//!
//! Runs on a clone of the registry and returns everything the UI thread needs
//! to apply the result; see `background_refresh` for the scheduling and ingest
//! side.

use std::{collections::HashSet, path::PathBuf, time::SystemTime};

use crate::{
    config::Config, discover, git, model::OrphanSession,
    overseer::discord_channels::DiscordChannels, registry::Registry,
    subagents::claude::ClaudeSubagentReader,
};

use super::{
    background_support::fingerprint,
    children, discovery,
    dropr_overlay::{self, OverlayStatus},
    orphans, registry_sync, subagents,
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
    // Take the stored row set before anything reads or prunes this snapshot, so
    // the whole pass — adoption, subagent ingest, orphan discovery — sees the
    // agents that still exist rather than ones another process already removed.
    // The fingerprint above deliberately stays the pre-reconcile one: it guards
    // the hand-off back to the UI thread, which knows nothing of this read. A
    // failed read leaves the snapshot alone rather than emptying the tree, and a
    // successful one needs no save — disk is where these rows came from.
    if let Ok(stored) = Registry::locked_load() {
        registry_sync::adopt_stored_agents(&mut registry.repos, &stored);
    }
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
    // Reloaded fresh rather than threaded in from the caller: this pass runs
    // off-thread on its own `Config`/`Registry` snapshot, and discord channel
    // session names are the one thing orphan discovery needs from a source
    // this pass otherwise never touches. See `overseer_refresh::capture_overseer`
    // for the same reload pattern applied to the OVERSEER frame's snapshot.
    let discord_channels = crate::overseer::discord_ops_dir()
        .ok()
        .and_then(|dir| DiscordChannels::load(&dir.join("channels.json")).ok())
        .unwrap_or_default();
    let found_orphans = orphans::discover_orphans(
        &registry.repos,
        &config.tmux_session_prefix,
        &config.worktree_root,
        &discord_channels,
    );
    DiscoveryResult {
        registry,
        fingerprint,
        orphans: found_orphans,
        save: repos_changed || added || removed,
        overlay,
    }
}
