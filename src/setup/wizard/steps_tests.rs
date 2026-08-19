use std::io::Cursor;

use crate::config::Config;

use super::steps;

#[test]
fn overseer_all_enter_leaves_configuration_unchanged() {
    let mut config = Config::default();
    let before = serde_json::to_vec(&config).unwrap();
    let mut input = Cursor::new(b"\n\n\n\n\n\n\n");
    steps::overseer(&mut input, &mut Vec::new(), &mut config).unwrap();
    assert_eq!(serde_json::to_vec(&config).unwrap(), before);
}

#[test]
fn unavailable_current_profile_is_preserved_by_default() {
    let mut config = Config::default();
    config.overseer.worker_profile = Some("custom-missing".into());
    let mut input = Cursor::new(b"\n\n\n\n\n\n\n");
    steps::overseer(&mut input, &mut Vec::new(), &mut config).unwrap();
    assert_eq!(
        config.overseer.worker_profile.as_deref(),
        Some("custom-missing")
    );
}

#[test]
fn ensure_agent_profiles_adds_installed_codex_to_empty_profiles() {
    let mut config = Config::default();
    config.profiles.clear();
    steps::ensure_agent_profiles_with(&mut config, |program| program == "codex");
    let names: Vec<&str> = config
        .profiles
        .iter()
        .map(|profile| profile.name.as_str())
        .collect();
    assert_eq!(names, ["codex"]);
    assert!(!config.profiles[0].autonomous_args.is_empty());
}

#[test]
fn ensure_agent_profiles_without_codex_leaves_profiles_unchanged() {
    let mut config = Config::default();
    config.profiles.clear();
    steps::ensure_agent_profiles_with(&mut config, |program| program == "claude");
    assert!(config.profiles.is_empty());
}

#[test]
fn ensure_agent_profiles_skips_existing_and_default_entries() {
    let mut config = Config::default();
    let before = config.profiles.clone();
    steps::ensure_agent_profiles_with(&mut config, |_| true);
    assert_eq!(config.profiles, before);
}

#[test]
fn ensure_agent_profiles_skips_custom_profile_with_same_program() {
    let mut config = Config::default();
    config.profiles.retain(|profile| profile.name != "codex");
    config.profiles.push(crate::config::Profile {
        name: "my-codex".into(),
        program: "codex".into(),
        autonomous_args: Vec::new(),
        model: None,
        backend: None,
    });
    let before = config.profiles.clone();
    steps::ensure_agent_profiles_with(&mut config, |_| true);
    assert_eq!(config.profiles, before);
}

#[test]
fn repaired_profiles_offer_codex_in_profile_steps() {
    let mut config = Config::default();
    config.profiles.clear();
    steps::ensure_agent_profiles_with(&mut config, |program| program == "codex");
    // auto-merge=n, worker=2 (codex), triage=default.
    let mut input = Cursor::new(b"n\n2\n\n");
    let mut output = Vec::new();
    steps::overseer(&mut input, &mut output, &mut config).unwrap();
    assert_eq!(config.overseer.worker_profile.as_deref(), Some("codex"));
    assert!(String::from_utf8(output).unwrap().contains("codex"));
}

#[test]
fn overseer_answers_are_applied() {
    let mut config = Config::default();
    let mut input = Cursor::new(b"y\n2\n3\n");
    let mut output = Vec::new();
    steps::overseer(&mut input, &mut output, &mut config).unwrap();
    assert!(config.overseer.auto_merge);
    assert_eq!(config.overseer.worker_profile.as_deref(), Some("claude"));
    assert_eq!(config.overseer.triage_profile.as_deref(), Some("codex"));
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("branch protection")
    );
}
