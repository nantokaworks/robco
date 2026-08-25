//! Resolving a repository's dropr remote URL from the path Overseer's
//! ledger and triage cases carry as `repo` (`LedgerEntry::repo`,
//! `ExceptionCase::repo`).
//!
//! A dropr write whose `task_id` is a `#N` display id — an entry adopted
//! from a live agent, per `LedgerEntry::dropr_task_id`'s doc comment — needs
//! a `repo_url` (or `workspace_id`) to scope the call, or dropr rejects it
//! outright. This resolves that `repo_url` from the local registry, with no
//! extra dropr round-trip (dropr:556). `spawn::resolve_repo`'s own path
//! matching is exercised in `spawn_tests.rs`; this is a thin composition on
//! top of it.

use crate::{registry::Registry, spawn};

pub(crate) fn repo_url_for(repo: &str) -> Option<String> {
    let registry = Registry::load().ok()?;
    spawn::resolve_repo(&registry, repo)
        .ok()
        .and_then(|node| node.remote_url.clone())
}
