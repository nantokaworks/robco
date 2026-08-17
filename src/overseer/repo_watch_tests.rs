use super::*;

fn now() -> DateTime<Utc> {
    "2026-08-17T00:00:00Z".parse().unwrap()
}

#[test]
fn a_missing_state_file_loads_as_empty() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("repo_watch.json");
    let state = RepoWatchState::load_from(&path).unwrap();
    assert!(state.repos.is_empty());
}

#[test]
fn a_saved_state_round_trips() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("repo_watch.json");
    let mut state = RepoWatchState::default();
    state.repos.insert("/repos/nex".into(), now());
    state.save_to(&path).unwrap();

    let reloaded = RepoWatchState::load_from(&path).unwrap();
    assert_eq!(reloaded, state);
}

#[test]
fn a_corrupt_state_file_loads_as_empty_rather_than_failing() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("repo_watch.json");
    fs::write(&path, "not json").unwrap();

    let state = RepoWatchState::load_from(&path).unwrap();
    assert!(state.repos.is_empty());
}
