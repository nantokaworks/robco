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

#[test]
fn merge_strategy_defaults_and_maps_to_gh_flags() {
    assert_eq!(MergeStrategy::default(), MergeStrategy::Rebase);
    assert_eq!(MergeStrategy::Rebase.gh_flag(), "--rebase");
    assert_eq!(MergeStrategy::Squash.gh_flag(), "--squash");
    assert_eq!(MergeStrategy::Merge.gh_flag(), "--merge");
}

#[test]
fn pr_prompt_defaults_when_missing_from_config() {
    let value = serde_json::to_value(Config::default()).unwrap();
    let mut object = value.as_object().unwrap().clone();
    object.remove("pr_prompt");
    let config: Config = serde_json::from_value(object.into()).unwrap();
    assert_eq!(config.pr_prompt, DEFAULT_PR_PROMPT);
}

#[test]
fn project_icon_defaults_and_maps_markers() {
    assert_eq!(ProjectIcon::default(), ProjectIcon::None);
    assert_eq!(ProjectIcon::None.marker(true), "▾");
    assert_eq!(ProjectIcon::None.marker(false), "▸");
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
