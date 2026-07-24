//! Keeping the TUI's view of each repository's dropr workspace current.
//!
//! The overlay is remote state — which workspaces exist and what they are
//! linked to — so it has to be reloaded on its own schedule. Tying the reload
//! to a change in the *local* repository set meant a stable set of
//! repositories never refreshed it, leaving `repo.dropr` empty for the rest of
//! the session.

use std::time::{Duration, Instant};

use crate::{dropr::DroprOverlay, model::RepoNode};

/// How long a loaded overlay is trusted before the TUI reloads it.
/// `dropr workspace list` is a subprocess, so the reload must not ride the
/// discovery cadence; a linkage changed outside robco surfaces within this
/// interval instead of requiring a restart.
const RELOAD_INTERVAL: Duration = Duration::from_secs(60);

/// What this process knows about the overlay. Without it, a repository with no
/// resolved workspace is indistinguishable from one whose overlay was never
/// loaded, and the TUI blames a linkage that was never broken.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum OverlayStatus {
    /// No load has completed in this process yet.
    #[default]
    Pending,
    /// `dropr workspace list` succeeded, so the resolved links are current.
    Loaded,
    /// The `dropr` CLI is missing, or the listing failed or timed out.
    Unavailable,
}

pub(super) fn reload_is_due(last_load_started: Option<Instant>, now: Instant) -> bool {
    last_load_started.is_none_or(|at| now.saturating_duration_since(at) >= RELOAD_INTERVAL)
}

/// Resolve every repository's workspace from a single overlay load, so the
/// subprocess runs once per refresh cycle rather than once per repository.
pub(super) fn load_and_apply(repos: &mut [RepoNode]) -> OverlayStatus {
    let (overlay, loaded) = DroprOverlay::load_with_status();
    if !loaded {
        // A failed listing says nothing about linkage. Keep whatever a previous
        // successful load resolved instead of reporting every repo unlinked.
        return OverlayStatus::Unavailable;
    }
    apply(repos, &overlay);
    OverlayStatus::Loaded
}

fn apply(repos: &mut [RepoNode], overlay: &DroprOverlay) {
    for repo in repos {
        repo.dropr = repo
            .remote_url
            .as_deref()
            .and_then(|remote| overlay.find_by_repo_url(remote))
            .cloned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &[u8] = b"kind          id                     name    repo_url\n  \
        materialised  11CZPXW_Hz2ySp3EP21BX  robco   https://github.com/nantokaworks/robco.git\n";

    fn repo(remote: Option<&str>) -> RepoNode {
        let mut repo = crate::discover::repo_node("/tmp/robco-overlay-test".into(), false);
        repo.remote_url = remote.map(str::to_owned);
        repo
    }

    #[test]
    fn reload_is_due_until_a_load_lands() {
        let now = Instant::now();

        assert!(reload_is_due(None, now));
        assert!(!reload_is_due(Some(now), now));
        assert!(reload_is_due(Some(now - RELOAD_INTERVAL), now));
    }

    #[test]
    fn apply_fills_an_empty_link_for_a_linked_repo() {
        let mut repos = vec![repo(Some("git@github.com:nantokaworks/robco.git"))];

        apply(&mut repos, &DroprOverlay::from_workspace_list(LISTING));

        let workspace = repos[0].dropr.as_ref().expect("workspace resolved");
        assert_eq!(workspace.id, "11CZPXW_Hz2ySp3EP21BX");
    }

    #[test]
    fn apply_clears_a_link_the_overlay_no_longer_carries() {
        let mut repos = vec![repo(Some("https://github.com/nantokaworks/other.git"))];
        repos[0].dropr = Some(crate::dropr::DroprWorkspace {
            kind: "materialised".into(),
            id: "unlinked-elsewhere".into(),
            name: "other".into(),
            repo_url: "https://github.com/nantokaworks/other.git".into(),
        });

        apply(&mut repos, &DroprOverlay::from_workspace_list(LISTING));

        assert!(repos[0].dropr.is_none());
    }

    #[test]
    fn apply_leaves_a_remoteless_repo_unlinked() {
        let mut repos = vec![repo(None)];

        apply(&mut repos, &DroprOverlay::from_workspace_list(LISTING));

        assert!(repos[0].dropr.is_none());
    }
}
