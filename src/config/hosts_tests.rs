use super::{Config, HostConfig};

#[test]
fn hosts_default_when_absent_and_round_trip() {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    value.as_object_mut().unwrap().remove("hosts");
    let old: Config = serde_json::from_value(value).unwrap();
    assert!(old.hosts.is_empty());

    let configured = Config {
        hosts: vec![
            HostConfig {
                ssh: "prod".into(),
                name: Some("Production".into()),
            },
            HostConfig {
                ssh: "dev@example".into(),
                name: None,
            },
        ],
        ..Config::default()
    };
    let decoded: Config =
        serde_json::from_str(&serde_json::to_string(&configured).unwrap()).unwrap();
    assert_eq!(decoded.hosts, configured.hosts);
}
