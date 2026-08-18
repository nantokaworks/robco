use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use super::{
    merge_concurrency, merge_pass_telemetry,
    merge_repo_pass::{self, RepoOutcome, RepoWork},
    merge_settle,
    protection::ProtectionCache,
};
use crate::{
    Result,
    config::Config,
    overseer::{
        ledger::{Ledger, LedgerEntry},
        logging::{self, DecisionEntry, DecisionKind},
        merge_pass_path,
    },
    registry::Registry,
};

/// Evaluates every repository's merge queue for one pass, up to
/// `max_concurrent_merge_repos` at a time.
///
/// Repositories are independent of one another — their base branches share
/// nothing — so each one's own sequential walk through its ledger entries
/// (`merge_repo_pass::run`) runs on its own worker thread; see
/// `merge_concurrency` for the bounded pool that schedules them and
/// `merge_repo_pass` for what one repository's walk actually does. What is
/// shared — the protection cache — is synchronised there; what is not
/// (`Heads`, each repository's settling barrier, each entry's own mutable
/// state) is extracted per repository before any worker starts and folded
/// back here once every worker has finished, so nothing is ever written from
/// two threads at once.
pub(super) fn auto_merge_pass(
    config: &Config,
    ledger: &mut Ledger,
    cache: &mut ProtectionCache,
    pulled: &HashSet<String>,
) -> Result<()> {
    if !config.overseer.auto_merge {
        return Ok(());
    }
    let started = Instant::now();
    let registry = Registry::load()?;
    let consecutive_failures = ledger.counters.consecutive_failures;
    // Merges are serialised per repository: a merge advances the base and leaves every
    // other pull request of that repository behind, so their reads from earlier in this
    // pass no longer describe a mergeable branch, and the primary checkout's local main
    // does not match the merge until the post-merge fast-forward lands. The barrier
    // outlives the pass because that fast-forward runs on a later one. Other
    // repositories stay independent.
    let max_settle_passes = config.overseer.max_merge_settle_passes;
    // Field-wise borrows: the barrier map is read and written while `entries` is grouped.
    let Ledger {
        entries,
        merge_settling,
        counters,
        ..
    } = ledger;
    for repo in merge_settle::settle(merge_settling, pulled) {
        log_repo(&repo, merge_settle::SETTLED)?;
    }
    merge_settle::age(merge_settling);
    let max_rechecks = config.overseer.max_merge_hold_rechecks;

    // Group entries by repository, preserving each repository's own relative
    // order — a repository's entries keep a stable order pass over pass (see
    // `merge_queue`'s doc comment), and that order is what decides which
    // entry is its head of queue this pass.
    let mut groups: HashMap<String, Vec<&mut LedgerEntry>> = HashMap::new();
    for entry in entries.iter_mut() {
        groups.entry(entry.repo.clone()).or_default().push(entry);
    }
    // Each repository's settling state is extracted into its own owned value
    // here, before any worker starts, so no two repositories' threads ever
    // touch `merge_settling` at once.
    let work: Vec<RepoWork> = groups
        .into_iter()
        .map(|(repo, entries)| {
            let settling = merge_settling.remove(&repo);
            RepoWork {
                repo,
                entries,
                settling,
            }
        })
        .collect();
    let repos_evaluated = work.len();

    let cache: &ProtectionCache = &*cache;
    let ceiling = config.overseer.max_concurrent_merge_repos;
    let outcomes = merge_concurrency::run_bounded(work, ceiling, |item| {
        merge_repo_pass::run(
            item,
            config,
            cache,
            &registry,
            consecutive_failures,
            max_rechecks,
            max_settle_passes,
        )
    });

    let mut merged_any = false;
    let mut slowest: Option<(String, std::time::Duration)> = None;
    for outcome in outcomes {
        let RepoOutcome {
            repo,
            settling,
            merged,
            duration,
        } = outcome?;
        merged_any = merged_any || merged;
        if let Some(settling) = settling {
            merge_settling.insert(repo.clone(), settling);
        }
        if slowest
            .as_ref()
            .is_none_or(|(_, slowest)| duration > *slowest)
        {
            slowest = Some((repo, duration));
        }
    }
    if merged_any {
        counters.consecutive_failures = 0;
    }

    merge_pass_telemetry::record(
        &merge_pass_path()?,
        chrono::Utc::now(),
        started.elapsed(),
        repos_evaluated,
        slowest,
    )?;
    Ok(())
}

/// Records a decision about a repository rather than about one of its entries.
///
/// The barrier coming down belongs to the repository — the merge that raised it
/// may have left the ledger by the time its pull lands — so there is no task id
/// to attribute it to, and inventing one from whichever entry happens to be
/// nearby would name a pull request that had nothing to do with it.
fn log_repo(repo: &str, reason: &str) -> Result<()> {
    let mut decision = DecisionEntry::new(DecisionKind::Hold, reason);
    decision.repo = Some(repo.to_owned());
    decision.source = Some("auto_merge".into());
    logging::append(&decision)
}
