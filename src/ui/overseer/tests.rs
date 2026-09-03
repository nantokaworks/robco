use super::*;
use crate::model::Status;

#[test]
fn status_tracks_daemon_liveness_only() {
    let mut snapshot = OverseerSnapshot {
        daemon_alive: false,
        ..Default::default()
    };
    assert_eq!(snapshot.status(), Status::Dead);

    snapshot.daemon_alive = true;
    assert_eq!(snapshot.status(), Status::Running);
}

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
fn health_warnings_report_liveness_and_version_drift() {
    assert_eq!(
        categories::health_warnings_from(false, false),
        ["STALE/OFFLINE"]
    );
    assert_eq!(
        categories::health_warnings_from(true, true),
        [crate::overseer::heartbeat::DRIFT_LABEL]
    );
}

#[test]
fn every_category_has_summary_detail_and_preview() {
    let temp = tempfile::tempdir().unwrap();
    let app = App::new(
        crate::registry::Registry::default(),
        crate::config::Config::default(),
        temp.path().into(),
    );
    for category in OverseerCategory::ALL {
        let (summary, _) = category_summary(&app, category);
        assert!(!summary.is_empty());
        let (title, _) = category_preview(&app, category);
        assert!(title.contains(category.label()));
    }
}
