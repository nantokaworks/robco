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

/// An older state file with no `reported_missing_tools` key at all — written
/// before dropr task #445 added the field — still loads instead of falling
/// back to `Default` the way a genuinely corrupt file does.
#[test]
fn a_state_file_without_the_missing_tools_field_still_loads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("repo_watch.json");
    fs::write(&path, r#"{"repos":{}}"#).unwrap();

    let state = RepoWatchState::load_from(&path).unwrap();
    assert!(state.reported_missing_tools.is_empty());
}

#[test]
fn reported_missing_tools_round_trips() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("repo_watch.json");
    let mut state = RepoWatchState::default();
    state.reported_missing_tools.insert("govulncheck".into());
    state.save_to(&path).unwrap();

    let reloaded = RepoWatchState::load_from(&path).unwrap();
    assert_eq!(reloaded, state);
}
