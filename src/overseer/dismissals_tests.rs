use chrono::TimeZone;

use super::*;

fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, second).unwrap()
}

fn live(targets: &[(&str, &str)]) -> HashSet<(String, String)> {
    targets
        .iter()
        .map(|(kind, target)| ((*kind).to_string(), (*target).to_string()))
        .collect()
}

#[test]
fn an_item_is_suppressed_by_its_identity_and_not_by_a_neighbour() {
    let mut dismissals = Dismissals::default();
    dismissals.dismiss("ESC", "#159", at(10));

    assert!(dismissals.suppresses("ESC", "#159", at(10)));
    assert!(dismissals.suppresses("ESC", "#159", at(9)));
    // Same target id, different kind: a question and an escalation about the
    // same worker are separate rows and clear separately.
    assert!(!dismissals.suppresses("?", "#159", at(10)));
    assert!(!dismissals.suppresses("ESC", "#160", at(10)));
}

#[test]
fn a_newer_escalation_for_a_dismissed_target_reappears() {
    let mut dismissals = Dismissals::default();
    dismissals.dismiss("ESC", "#159", at(10));

    // Anything the sources raise after the dismissal is a new alert. Hiding it
    // would mute the task for good, which is exactly what dismissal must not do.
    assert!(!dismissals.suppresses("ESC", "#159", at(11)));
}

#[test]
fn re_dismissing_carries_the_window_forward_and_never_back() {
    let mut dismissals = Dismissals::default();
    dismissals.dismiss("ESC", "#159", at(10));
    dismissals.dismiss("ESC", "#159", at(20));
    assert_eq!(dismissals.entries.len(), 1);
    assert!(dismissals.suppresses("ESC", "#159", at(20)));

    dismissals.dismiss("ESC", "#159", at(5));
    assert!(dismissals.suppresses("ESC", "#159", at(20)));
}

#[test]
fn pruning_drops_entries_no_source_still_produces() {
    let mut dismissals = Dismissals::default();
    dismissals.dismiss("ESC", "#159", at(10));
    dismissals.dismiss("ESC", "#160", at(10));

    dismissals.retain_live(|kind, target| (kind, target) == ("ESC", "#160"));

    assert_eq!(dismissals.entries.len(), 1);
    assert_eq!(dismissals.entries[0].target_id, "#160");
}

#[test]
fn a_dismissal_survives_a_reload() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inbox_dismissals.json");

    dismiss_at(&path, &[("ESC", "#159", at(10))], &live(&[("ESC", "#159")])).unwrap();

    let reloaded = Dismissals::load_from(&path).unwrap();
    assert!(reloaded.suppresses("ESC", "#159", at(10)));
    assert!(!reloaded.suppresses("ESC", "#159", at(11)));
}

#[test]
fn a_write_prunes_targets_that_are_gone_but_keeps_the_one_being_dismissed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inbox_dismissals.json");

    dismiss_at(&path, &[("ESC", "#159", at(10))], &live(&[("ESC", "#159")])).unwrap();
    // #159 has since aged out of every source; #160 is the row being dismissed
    // now. Only the live identity survives.
    dismiss_at(&path, &[("ESC", "#160", at(20))], &live(&[("ESC", "#160")])).unwrap();

    let reloaded = Dismissals::load_from(&path).unwrap();
    assert_eq!(reloaded.entries.len(), 1);
    assert_eq!(reloaded.entries[0].target_id, "#160");
}

#[test]
fn a_dismissal_survives_a_live_set_that_does_not_mention_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inbox_dismissals.json");

    // The row being dismissed is live by definition. Pruning must not depend on
    // the caller's set agreeing, or an empty one would discard the write.
    dismiss_at(&path, &[("ESC", "#159", at(10))], &live(&[])).unwrap();

    assert!(
        Dismissals::load_from(&path)
            .unwrap()
            .suppresses("ESC", "#159", at(10))
    );
}

#[test]
fn a_missing_or_corrupt_list_reads_as_empty() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inbox_dismissals.json");
    assert_eq!(Dismissals::load_from(&path).unwrap(), Dismissals::default());

    std::fs::write(&path, b"{ not json").unwrap();
    assert_eq!(Dismissals::load_from(&path).unwrap(), Dismissals::default());
}
