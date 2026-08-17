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
