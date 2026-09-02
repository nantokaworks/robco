use super::*;

#[test]
fn publish_tags_repos_and_preserves_last_success_on_error() {
    let cell = Mutex::new(HostSnapshot::default());
    let label = HostLabel {
        name: "Prod".into(),
        ssh: "prod".into(),
    };
    let repo = serde_json::from_value(serde_json::json!({
        "path": "/srv/repo", "name": "repo", "remote_url": null
    }))
    .unwrap();
    publish(&cell, &label, vec![repo], None);
    publish_error(&cell, "offline".into());
    let snapshot = cell.lock().unwrap();
    assert_eq!(snapshot.repos[0].host.as_ref(), Some(&label));
    assert_eq!(snapshot.repos.len(), 1);
    assert_eq!(snapshot.error.as_deref(), Some("offline"));
}

#[test]
fn connection_tracks_first_success_and_failures() {
    let label = HostLabel {
        name: "Prod".into(),
        ssh: "prod".into(),
    };
    let slot = HostSlot::idle(label.clone());
    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Connecting, None)
    );

    publish(&slot.snapshot, &label, Vec::new(), None);
    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Connected, None)
    );

    publish_error(&slot.snapshot, "offline".into());
    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Failed, Some("offline".into()))
    );
}

#[test]
fn failed_first_publish_is_failed_despite_advanced_generation() {
    let slot = HostSlot::idle(HostLabel {
        name: "Prod".into(),
        ssh: "prod".into(),
    });
    publish_error(&slot.snapshot, "offline".into());
    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Failed, Some("offline".into()))
    );
}

#[test]
fn poisoned_snapshot_remains_readable_and_writable() {
    let label = HostLabel {
        name: "Prod".into(),
        ssh: "prod".into(),
    };
    let slot = HostSlot::idle(label.clone());
    let snapshot = Arc::clone(&slot.snapshot);
    let _ = std::panic::catch_unwind(|| {
        let _guard = snapshot.lock().unwrap();
        panic!("poison snapshot");
    });

    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Connecting, None)
    );
    assert!(slot.backend().is_none());
    publish(&slot.snapshot, &label, Vec::new(), None);
    publish_error(&slot.snapshot, "offline".into());
    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Failed, Some("offline".into()))
    );
}
