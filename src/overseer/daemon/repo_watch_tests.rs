use super::*;

fn now() -> DateTime<Utc> {
    "2026-08-17T00:00:00Z".parse().unwrap()
}

#[test]
fn a_repository_never_checked_is_due() {
    assert!(due(None, now(), chrono::Duration::hours(24)));
}

#[test]
fn a_repository_checked_within_the_interval_is_not_due() {
    let checked_at = now() - chrono::Duration::hours(1);
    assert!(!due(Some(&checked_at), now(), chrono::Duration::hours(24)));
}

#[test]
fn a_repository_checked_before_the_interval_is_due_again() {
    let checked_at = now() - chrono::Duration::hours(24);
    assert!(due(Some(&checked_at), now(), chrono::Duration::hours(24)));
}

fn registry_with(path: &str) -> Registry {
    Registry {
        version: 1,
        repos: vec![crate::discover::repo_node(path.into(), false)],
    }
}

#[test]
fn prune_unregistered_drops_a_path_the_registry_no_longer_lists() {
    let mut state = RepoWatchState::default();
    state.repos.insert("/repos/renamed-away".into(), now());
    state.repos.insert("/repos/robco".into(), now());

    prune_unregistered(&mut state, &registry_with("/repos/robco"));

    assert_eq!(state.repos.keys().collect::<Vec<_>>(), vec!["/repos/robco"]);
}

#[test]
fn prune_unregistered_keeps_every_registered_path() {
    let mut state = RepoWatchState::default();
    state.repos.insert("/repos/robco".into(), now());

    prune_unregistered(&mut state, &registry_with("/repos/robco"));

    assert_eq!(state.repos.len(), 1);
}
