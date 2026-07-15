use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Registry agent identity inherited by processes in an agent tmux session.
pub const ENV_AGENT_ID: &str = "ROBCO_AGENT_ID";
/// Optional controller identity; no current creation flow knows a parent id.
pub const ENV_PARENT_AGENT_ID: &str = "ROBCO_PARENT_AGENT_ID";

const DEFAULT_PR_PROMPT: &str = "Commit any remaining changes, push the branch, and open a pull request against main following the project's PR conventions.";

fn default_pr_prompt() -> String {
    DEFAULT_PR_PROMPT.to_string()
}

use crate::{Result, chief::config::ChiefConfig, model::Status, openclaw::OpenClawConfig};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MergeStrategy {
    #[default]
    Rebase,
    Squash,
    Merge,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectIcon {
    #[default]
    None,
    Nerdfont,
    Emoji,
}

impl ProjectIcon {
    /// PROJECTS 行の開閉マーカー。None は従来の三角、その他はフォルダの開/閉。
    pub fn marker(self, expanded: bool) -> &'static str {
        match (self, expanded) {
            (ProjectIcon::None, true) => "▾",
            (ProjectIcon::None, false) => "▸",
            (ProjectIcon::Nerdfont, true) => "\u{f07c}", // nf-fa-folder_open
            (ProjectIcon::Nerdfont, false) => "\u{f07b}", // nf-fa-folder
            (ProjectIcon::Emoji, true) => "📂",
            (ProjectIcon::Emoji, false) => "📁",
        }
    }
}

impl MergeStrategy {
    pub fn gh_flag(self) -> &'static str {
        match self {
            MergeStrategy::Rebase => "--rebase",
            MergeStrategy::Squash => "--squash",
            MergeStrategy::Merge => "--merge",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotifyConfig {
    pub enabled: bool,
    pub waiting: bool,
    pub idle: bool,
    /// Notify when the AI finishes a turn (`Status::Done`). Defaults to on.
    #[serde(default = "notify_flag_default")]
    pub done: bool,
    pub dead: bool,
}

fn notify_flag_default() -> bool {
    true
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            waiting: true,
            idle: true,
            done: true,
            dead: true,
        }
    }
}

impl NotifyConfig {
    pub fn wants(&self, status: Status) -> bool {
        if !self.enabled {
            return false;
        }

        match status {
            Status::Waiting => self.waiting,
            Status::Idle => self.idle,
            Status::Done => self.done,
            Status::Dead => self.dead,
            Status::Running | Status::BranchOnly => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_program: String,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub branch_prefix: Option<String>,
    pub worktree_root: PathBuf,
    pub tmux_session_prefix: String,
    pub poll_interval_ms: u64,
    pub dropr_overlay: bool,
    #[serde(default)]
    pub auto_accept: bool,
    #[serde(default = "notify_flag_default")]
    pub process_indicator: bool,
    #[serde(default = "notify_flag_default")]
    pub subagent_indicator: bool,
    #[serde(default)]
    pub merge_strategy: MergeStrategy,
    /// Prompt sent to an agent when requesting a PR. Defaults to "Commit any
    /// remaining changes, push the branch, and open a pull request against main
    /// following the project's PR conventions."
    #[serde(default = "default_pr_prompt")]
    pub pr_prompt: String,
    #[serde(default)]
    pub notify: NotifyConfig,
    #[serde(default)]
    pub openclaw: OpenClawConfig,
    #[serde(default)]
    pub project_icon: ProjectIcon,
    #[serde(default)]
    pub chief: ChiefConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub autonomous_args: Vec<String>,
}

fn default_profiles() -> Vec<Profile> {
    vec![
        Profile {
            name: "claude".to_string(),
            program: "claude".to_string(),
            autonomous_args: vec!["--dangerously-skip-permissions".to_string()],
        },
        Profile {
            name: "codex".to_string(),
            program: "codex".to_string(),
            autonomous_args: vec!["--dangerously-bypass-approvals-and-sandbox".to_string()],
        },
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_program: "claude".to_string(),
            profiles: default_profiles(),
            branch_prefix: None,
            worktree_root: home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".robco")
                .join("worktrees"),
            tmux_session_prefix: "robco_".to_string(),
            poll_interval_ms: 750,
            dropr_overlay: true,
            auto_accept: false,
            process_indicator: true,
            subagent_indicator: true,
            merge_strategy: MergeStrategy::default(),
            pr_prompt: default_pr_prompt(),
            notify: NotifyConfig::default(),
            openclaw: OpenClawConfig::default(),
            project_icon: ProjectIcon::default(),
            chief: ChiefConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(path)?;
        let mut config: Config = serde_json::from_str(&raw)?;
        if config.default_program.trim().is_empty() {
            config.default_program = "claude".to_string();
        }
        // Expand a user-written `~` so paths match git's absolute worktree paths
        // and are not re-adopted as duplicates.
        config.worktree_root = expand_tilde(&config.worktree_root);
        Ok(config)
    }

    /// Persist the config to `~/.robco/config.json`, mirroring [`Registry::save`].
    pub fn save(&self) -> Result<()> {
        ensure_robco_dir()?;
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(config_path()?, raw)?;
        Ok(())
    }

    pub fn default_program_command(&self) -> String {
        self.profiles
            .iter()
            .find(|profile| profile.name == self.default_program)
            .map(|profile| profile.program.clone())
            .unwrap_or_else(|| self.default_program.clone())
    }
}

pub fn state_path() -> Result<PathBuf> {
    Ok(robco_dir()?.join("state.json"))
}

pub fn ensure_robco_dir() -> Result<PathBuf> {
    let dir = robco_dir()?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn config_path() -> Result<PathBuf> {
    Ok(robco_dir()?.join("config.json"))
}

pub fn config_file_path() -> Result<PathBuf> {
    config_path()
}

pub(crate) fn robco_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or(crate::Error::HomeDir)?;
    Ok(home.join(".robco"))
}

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Expand a leading `~` component to the home directory. Paths without a `~`
/// prefix, and paths that cannot be expanded (no home dir), are returned as-is.
fn expand_tilde(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => match home_dir() {
            Some(home) => home.join(rest),
            None => path.to_path_buf(),
        },
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_default_program_through_profiles() {
        let config = Config {
            default_program: "codex".to_string(),
            profiles: vec![Profile {
                name: "codex".to_string(),
                program: "codex --ask-for-approval never".to_string(),
                autonomous_args: Vec::new(),
            }],
            ..Config::default()
        };

        assert_eq!(
            config.default_program_command(),
            "codex --ask-for-approval never"
        );
    }

    #[test]
    fn legacy_config_defaults_chief_and_profile_autonomous_args() {
        let mut value = serde_json::to_value(Config::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("chief");
        for profile in object["profiles"].as_array_mut().unwrap() {
            profile.as_object_mut().unwrap().remove("autonomous_args");
        }

        let config: Config = serde_json::from_value(value).unwrap();
        assert_eq!(config.chief, ChiefConfig::default());
        assert!(
            config
                .profiles
                .iter()
                .all(|profile| profile.autonomous_args.is_empty())
        );
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
    fn merge_strategy_defaults_to_rebase_and_maps_to_gh_flags() {
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
    fn project_icon_defaults_to_none_and_maps_markers() {
        assert_eq!(ProjectIcon::default(), ProjectIcon::None);
        assert_eq!(ProjectIcon::None.marker(true), "▾");
        assert_eq!(ProjectIcon::None.marker(false), "▸");
        assert_eq!(ProjectIcon::Nerdfont.marker(true), "\u{f07c}");
        assert_eq!(ProjectIcon::Nerdfont.marker(false), "\u{f07b}");
        assert_eq!(ProjectIcon::Emoji.marker(true), "📂");
        assert_eq!(ProjectIcon::Emoji.marker(false), "📁");
    }

    #[test]
    fn expand_tilde_resolves_leading_home_and_passes_through_absolute() {
        let home = home_dir().expect("home dir");

        assert_eq!(
            expand_tilde(Path::new("~/.robco/worktrees")),
            home.join(".robco").join("worktrees")
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
}
