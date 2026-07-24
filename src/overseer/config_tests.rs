use super::*;

#[test]
fn loads_a_config_that_still_carries_the_removed_enabled_key() {
    let raw = r#"{"enabled": false, "dispatch_enabled": true, "max_workers": 5}"#;
    let config: OverseerConfig = serde_json::from_str(raw).unwrap();
    assert!(config.dispatch_enabled);
    assert_eq!(config.max_workers, 5);
}

#[test]
fn serialized_config_carries_no_enabled_key() {
    let value = serde_json::to_value(OverseerConfig::default()).unwrap();
    assert!(value.get("enabled").is_none());
    assert!(value.get("dispatch_enabled").is_some());
}
