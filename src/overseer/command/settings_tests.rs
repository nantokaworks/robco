use super::*;

#[test]
fn autonomy_level_round_trips_every_level() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.json");
    Config::default().save_at(&path).unwrap();

    for level in [
        AutonomyLevel::FullAuto,
        AutonomyLevel::ApprovalOnly,
        AutonomyLevel::Conservative,
    ] {
        persist_autonomy_level_at(&path, level).unwrap();
        assert_eq!(
            Config::load_at(&path).unwrap().overseer.autonomy_level,
            level
        );
    }
}

#[test]
fn autonomy_level_keeps_a_concurrent_edit_to_another_field() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.json");
    Config::default().save_at(&path).unwrap();

    // Stand in for the daemon rewriting the file after the CLI read its own
    // snapshot: the write must reload rather than serialise a stale one.
    let mut concurrent = Config::load_at(&path).unwrap();
    concurrent.overseer.max_workers = 11;
    concurrent.save_at(&path).unwrap();

    persist_autonomy_level_at(&path, AutonomyLevel::FullAuto).unwrap();

    let after = Config::load_at(&path).unwrap();
    assert_eq!(after.overseer.autonomy_level, AutonomyLevel::FullAuto);
    assert_eq!(after.overseer.max_workers, 11);
}

#[test]
fn only_the_widened_level_warns() {
    // `approval_only` escalates more than the default, so a warning there would
    // describe a gate that got tighter — the opposite of what it says.
    assert!(AutonomyLevel::ApprovalOnly.envelope_warning().is_none());
    assert!(AutonomyLevel::Conservative.envelope_warning().is_none());
    let warning = AutonomyLevel::FullAuto.envelope_warning().unwrap();
    for no_longer_escalated in ["ambiguous", "dependency", "large diff", "prod/CI-config"] {
        assert!(
            warning.contains(no_longer_escalated),
            "warning must name {no_longer_escalated}: {warning}"
        );
    }
}
