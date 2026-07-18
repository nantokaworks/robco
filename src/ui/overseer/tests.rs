use super::*;

#[test]
fn stale_heartbeat_is_not_fresh() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("heartbeat");
    fs::write(&path, "tick").unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(heartbeat_is_fresh_at(
        &path,
        10,
        modified + Duration::from_secs(20)
    ));
    assert!(!heartbeat_is_fresh_at(
        &path,
        10,
        modified + Duration::from_secs(21)
    ));
    assert!(!heartbeat_is_fresh_at(
        &temp.path().join("missing"),
        10,
        modified
    ));
}

#[test]
fn stale_dispatch_counter_renders_zero() {
    let today = Utc::now().date_naive();
    let mut ledger = Ledger::default();
    ledger.counters.date = today.pred_opt();
    ledger.counters.dispatched_today = 7;
    let mut lines = Vec::new();
    append_ledger(&mut lines, &OverseerConfig::default(), &ledger);
    let rendered = lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(rendered.starts_with("dispatches today: 0 / "));
}
