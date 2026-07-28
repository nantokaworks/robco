use super::*;

#[test]
fn resolves_default_program_through_profiles() {
    let config = Config {
        default_program: "codex".into(),
        profiles: vec![Profile {
            name: "codex".into(),
            program: "codex --ask-for-approval never".into(),
            autonomous_args: Vec::new(),
            model: None,
            backend: None,
        }],
        ..Config::default()
    };
    assert_eq!(
        config.default_program_command(),
        "codex --ask-for-approval never"
    );
}

#[test]
fn legacy_config_defaults_new_fields() {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("overseer");
    for profile in object["profiles"].as_array_mut().unwrap() {
        let profile = profile.as_object_mut().unwrap();
        profile.remove("autonomous_args");
        profile.remove("model");
        profile.remove("backend");
    }
    let config: Config = serde_json::from_value(value).unwrap();
    assert_eq!(config.overseer, OverseerConfig::default());
    assert!(config.profiles.iter().all(|profile| {
        profile.autonomous_args.is_empty() && profile.model.is_none() && profile.backend.is_none()
    }));
}

#[test]
fn legacy_chief_config_deserializes_and_serializes_as_overseer() {
    let expected = OverseerConfig {
        auto_merge: true,
        max_workers: 9,
        ..OverseerConfig::default()
    };
    let mut value = serde_json::to_value(Config::default()).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("overseer");
    object.insert("chief".into(), serde_json::to_value(&expected).unwrap());
    let config: Config = serde_json::from_value(value).unwrap();
    assert_eq!(config.overseer, expected);
    let serialized = serde_json::to_value(config).unwrap();
    assert!(serialized.get("chief").is_none());
    assert_eq!(
        serialized.get("overseer"),
        Some(&serde_json::to_value(expected).unwrap())
    );
}

#[test]
fn legacy_config_defaults_repos_root() {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    value.as_object_mut().unwrap().remove("repos_root");
    let config: Config = serde_json::from_value(value).unwrap();
    assert_eq!(config.repos_root, default_repos_root());
}

#[test]
fn built_in_profiles_have_autonomous_defaults() {
    let config = Config::default();
    assert_eq!(
        config.profiles[0].autonomous_args,
        ["--dangerously-skip-permissions"]
    );
    assert_eq!(
        config.profiles[1].autonomous_args,
        ["--dangerously-bypass-approvals-and-sandbox"]
    );
}

/// A config file carrying the top-level `merge_strategy`, the retired
/// `overseer.merge_strategy`, both, or neither — the four shapes on disk that
/// the one setting has to resolve the same way for every merge path.
fn config_json(top_level: Option<&str>, legacy: Option<&str>) -> serde_json::Value {
    let mut value = serde_json::to_value(Config::default()).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("merge_strategy");
    if let Some(top_level) = top_level {
        object.insert("merge_strategy".into(), top_level.into());
    }
    if let Some(legacy) = legacy {
        let overseer = object["overseer"].as_object_mut().unwrap();
        overseer.insert("merge_strategy".into(), legacy.into());
    }
    value
}

fn load_written(dir: &Path, top_level: Option<&str>, legacy: Option<&str>) -> Config {
    let path = dir.join("config.json");
    let raw = serde_json::to_string(&config_json(top_level, legacy)).unwrap();
    fs::write(&path, raw).unwrap();
    Config::load_at(&path).unwrap()
}

#[test]
fn a_config_naming_no_merge_strategy_takes_the_shared_default() {
    let temp = tempfile::tempdir().unwrap();
    let config = load_written(temp.path(), None, None);
    assert_eq!(config.merge_strategy, MergeStrategy::Squash);
    assert_eq!(config.merge_strategy_notice, None);
}

#[test]
fn the_top_level_merge_strategy_alone_drives_both_paths() {
    let temp = tempfile::tempdir().unwrap();
    let config = load_written(temp.path(), Some("rebase"), None);
    assert_eq!(config.merge_strategy, MergeStrategy::Rebase);
    assert_eq!(config.merge_strategy_notice, None);
}

#[test]
fn the_legacy_overseer_merge_strategy_alone_is_adopted_and_reported() {
    let temp = tempfile::tempdir().unwrap();
    let config = load_written(temp.path(), None, Some("merge"));
    assert_eq!(config.merge_strategy, MergeStrategy::Merge);
    assert!(config.merge_strategy_notice.is_some());
    assert_eq!(config.overseer.legacy_merge_strategy, None);
}

/// The shape that produced the divergence: the daemon squashed while the TUI
/// rebased. One value survives, and the operator is told which.
#[test]
fn conflicting_merge_strategy_keys_resolve_to_one_value_and_report_it() {
    let temp = tempfile::tempdir().unwrap();
    let config = load_written(temp.path(), Some("rebase"), Some("squash"));
    assert_eq!(config.merge_strategy, MergeStrategy::Squash);
    let notice = config
        .merge_strategy_notice
        .expect("the conflict is reported");
    assert!(notice.contains("overseer.merge_strategy"), "{notice}");
}

/// The migration is only finished once the file stops carrying the key that can
/// drift, so the next save has to drop it.
#[test]
fn the_legacy_overseer_key_does_not_survive_a_save() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.json");
    let config = load_written(temp.path(), Some("rebase"), Some("merge"));
    config.save_at(&path).unwrap();

    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(saved["overseer"].get("merge_strategy").is_none());
    assert_eq!(saved["merge_strategy"], serde_json::json!("merge"));
}

#[test]
fn pr_prompt_defaults_when_missing_from_config() {
    let value = serde_json::to_value(Config::default()).unwrap();
    let mut object = value.as_object().unwrap().clone();
    object.remove("pr_prompt");
    let config: Config = serde_json::from_value(object.into()).unwrap();
    assert_eq!(config.pr_prompt, DEFAULT_PR_PROMPT);
}

/// A config written before the key existed has to keep loading, and has to load
/// as "not configured" rather than as some default language.
#[test]
fn language_defaults_to_unset_when_missing_from_config() {
    let value = serde_json::to_value(Config::default()).unwrap();
    let mut object = value.as_object().unwrap().clone();
    object.remove("language");
    let config: Config = serde_json::from_value(object.into()).unwrap();
    assert_eq!(config.language, None);
}

/// The key is written and read through the file, so a value set in the TUI's
/// `$EDITOR` pass survives the save the daemon reloads from.
#[test]
fn a_configured_language_survives_a_save_and_load_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.json");
    let config = Config {
        language: Some("日本語".into()),
        ..Config::default()
    };
    config.save_at(&path).unwrap();
    assert_eq!(
        Config::load_at(&path).unwrap().language,
        Some("日本語".into())
    );
}

#[test]
fn project_icon_defaults_and_maps_markers() {
    assert_eq!(ProjectIcon::default(), ProjectIcon::None);
    assert_eq!(ProjectIcon::None.marker(true), "▾");
    assert_eq!(ProjectIcon::None.marker(false), "▽");
    assert_eq!(ProjectIcon::Nerdfont.marker(true), "\u{f07c}");
    assert_eq!(ProjectIcon::Nerdfont.marker(false), "\u{f07b}");
    assert_eq!(ProjectIcon::Emoji.marker(true), "📂");
    assert_eq!(ProjectIcon::Emoji.marker(false), "📁");
}

#[test]
fn expand_tilde_resolves_home_and_passes_through_absolute() {
    let home = home_dir().expect("home dir");
    assert_eq!(
        expand_tilde(Path::new("~/.robco/worktrees")),
        home.join(".robco/worktrees")
    );
    assert_eq!(expand_tilde(Path::new("~")), home);
    let absolute = PathBuf::from("/tmp/robco/worktrees");
    assert_eq!(expand_tilde(&absolute), absolute);
}

#[test]
fn project_icon_serde_roundtrip_is_lowercase() {
    assert_eq!(
        serde_json::to_string(&ProjectIcon::Nerdfont).unwrap(),
        "\"nerdfont\""
    );
    assert_eq!(
        serde_json::from_str::<ProjectIcon>("\"emoji\"").unwrap(),
        ProjectIcon::Emoji
    );
    assert_eq!(
        serde_json::from_str::<ProjectIcon>("\"none\"").unwrap(),
        ProjectIcon::None
    );
}
