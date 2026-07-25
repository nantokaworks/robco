use super::*;
use crate::config::MergeStrategy;

fn seeded(path: &Path) -> Config {
    let config = Config::default();
    config.save_at(path).unwrap();
    config
}

#[test]
fn operator_edit_during_a_pass_survives_the_daemon_write_back() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.json");
    // The snapshot the pass loaded at its top, before the long-running passes.
    let stale = seeded(&path);

    // The operator edits fields the daemon does not own while the pass runs.
    let mut edited = Config::load_at(&path).unwrap();
    edited.overseer.max_workers = 9;
    edited.poll_interval_ms = 1234;
    edited.merge_strategy = MergeStrategy::Squash;
    edited.save_at(&path).unwrap();

    assert!(persist_dispatch_enabled_at(&path, !stale.overseer.dispatch_enabled).unwrap());

    let after = Config::load_at(&path).unwrap();
    assert_eq!(after.overseer.max_workers, 9);
    assert_eq!(after.poll_interval_ms, 1234);
    assert_eq!(after.merge_strategy, MergeStrategy::Squash);
    assert_eq!(
        after.overseer.dispatch_enabled,
        !stale.overseer.dispatch_enabled
    );
}

#[test]
fn open_circuit_write_persists_dispatch_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.json");
    let seeded = seeded(&path);
    assert!(seeded.overseer.dispatch_enabled);

    assert!(persist_dispatch_enabled_at(&path, false).unwrap());

    assert!(!Config::load_at(&path).unwrap().overseer.dispatch_enabled);
}

#[test]
fn a_reenabled_dispatch_is_disabled_again_by_a_later_circuit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.json");
    seeded(&path);

    assert!(persist_dispatch_enabled_at(&path, false).unwrap());
    let mut reenabled = Config::load_at(&path).unwrap();
    reenabled.overseer.dispatch_enabled = true;
    reenabled.save_at(&path).unwrap();

    // Narrowing the write to one field must not make the circuit skippable:
    // the disable is decided against the current file, not a cached value.
    assert!(persist_dispatch_enabled_at(&path, false).unwrap());
    assert!(!Config::load_at(&path).unwrap().overseer.dispatch_enabled);
}

#[test]
fn a_value_already_on_disk_is_not_rewritten() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.json");
    seeded(&path);

    assert!(!persist_dispatch_enabled_at(&path, true).unwrap());
    assert!(Config::load_at(&path).unwrap().overseer.dispatch_enabled);
}
