use super::*;
use crate::config::Config;

fn config_with(session_env: &[(&str, &str)]) -> Config {
    Config {
        overseer: crate::overseer::config::OverseerConfig {
            session_env: session_env
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            ..Default::default()
        },
        ..Config::default()
    }
}

#[test]
fn config_assignment_wins_over_env_file() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("env");
    fs::write(&file, "CLAUDE_CODE_OAUTH_TOKEN=from-file\nOTHER=kept\n").unwrap();

    let env = SessionEnv::resolve_at(
        &config_with(&[("CLAUDE_CODE_OAUTH_TOKEN", "from-config")]),
        Some(&file),
    );

    assert_eq!(
        env.pairs(),
        vec![
            (
                "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
                "from-config".to_string()
            ),
            ("OTHER".to_string(), "kept".to_string()),
        ]
    );
    assert_eq!(
        env.credential(),
        Some(Credential {
            name: "CLAUDE_CODE_OAUTH_TOKEN".into(),
            source: EnvSource::Config,
        })
    );
}

#[test]
fn missing_env_file_leaves_only_config_assignments() {
    let temp = tempfile::tempdir().unwrap();

    let env = SessionEnv::resolve_at(
        &config_with(&[("ANTHROPIC_API_KEY", "key")]),
        Some(&temp.path().join("absent")),
    );

    assert_eq!(
        env.pairs(),
        vec![("ANTHROPIC_API_KEY".to_string(), "key".to_string())]
    );
    assert_eq!(
        env.credential_with(|_| None).unwrap().source,
        EnvSource::Config
    );
}

#[test]
fn inherited_credential_is_reported_when_nothing_is_configured() {
    let env = SessionEnv::default();

    assert!(env.pairs().is_empty());
    assert_eq!(
        env.credential_with(|name| (name == "ANTHROPIC_API_KEY").then(|| "inherited".to_string())),
        Some(Credential {
            name: "ANTHROPIC_API_KEY".into(),
            source: EnvSource::Process,
        })
    );
    assert_eq!(env.credential_with(|_| None), None);
}

#[test]
fn blank_credential_value_does_not_count_as_configured() {
    let env = SessionEnv::resolve_at(&config_with(&[("CLAUDE_CODE_OAUTH_TOKEN", "  ")]), None);

    assert_eq!(env.credential_with(|_| None), None);
    assert_eq!(env.credential_with(|_| Some(String::new())), None);
}

#[test]
fn env_file_parsing_matches_the_environment_file_convention() {
    let parsed = parse_env_file(
        "# comment\n\nexport CLAUDE_CODE_OAUTH_TOKEN=\"quoted value\"\nPLAIN=value\nSINGLE='single'\n  SPACED = spaced \nnot-a-name=skipped\nnovalue\n=empty\n",
    );

    assert_eq!(
        parsed,
        vec![
            (
                "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
                "quoted value".to_string()
            ),
            ("PLAIN".to_string(), "value".to_string()),
            ("SINGLE".to_string(), "single".to_string()),
            ("SPACED".to_string(), "spaced".to_string()),
        ]
    );
}

#[test]
fn empty_config_names_are_ignored() {
    let env = SessionEnv::resolve_at(&config_with(&[("   ", "value")]), None);

    assert!(env.pairs().is_empty());
}

#[test]
fn configured_names_are_listed_for_the_blocklist_exemption() {
    let env = SessionEnv::resolve_at(
        &config_with(&[("CLAUDE_CODE_OAUTH_TOKEN", "token"), ("B", "b")]),
        None,
    );

    assert_eq!(
        env.names().collect::<Vec<_>>(),
        vec!["B", "CLAUDE_CODE_OAUTH_TOKEN"]
    );
}

#[test]
fn env_file_path_prefers_the_configured_location() {
    let mut config = Config::default();
    config.overseer.session_env_file = Some("/tmp/robco-session-env".into());

    assert_eq!(
        env_file_path(&config),
        Some(PathBuf::from("/tmp/robco-session-env"))
    );

    config.overseer.session_env_file = None;
    assert!(env_file_path(&config).unwrap().ends_with(".robco/env"));
}

#[test]
fn apply_sets_every_assignment_on_the_command() {
    let env = SessionEnv::resolve_at(&config_with(&[("ROBCO_TEST_ENV", "applied")]), None);
    let mut command = Command::new("/bin/sh");
    env.apply(&mut command);

    assert!(
        command
            .get_envs()
            .any(|(name, value)| name == "ROBCO_TEST_ENV"
                && value == Some(std::ffi::OsStr::new("applied")))
    );
}
