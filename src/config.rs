mod discord_notify;
mod language;
mod merge_strategy;
mod notify;
mod paths;

use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) use language::directive as language_directive;
pub use merge_strategy::MergeStrategy;
pub use notify::NotifyConfig;
use paths::config_path;
pub use paths::{config_file_path, ensure_robco_dir, state_path};
pub(crate) use paths::{expand_tilde, home_dir, robco_dir, ui_state_path};

pub(crate) fn resolve_program(name: &str) -> Option<PathBuf> {
    crate::overseer::session::resolve_program_impl(name)
}

use nanoid::nanoid;
use serde::{Deserialize, Serialize};

/// Registry agent identity inherited by processes in an agent tmux session.
pub const ENV_AGENT_ID: &str = "ROBCO_AGENT_ID";
/// Optional controller identity; no current creation flow knows a parent id.
pub const ENV_PARENT_AGENT_ID: &str = "ROBCO_PARENT_AGENT_ID";

const DEFAULT_PR_PROMPT: &str = "Commit any remaining changes, push the branch, and open a pull request against main following the project's PR conventions.";

fn default_pr_prompt() -> String {
    DEFAULT_PR_PROMPT.to_string()
}

use crate::{Result, openclaw::OpenClawConfig, overseer::config::OverseerConfig};

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

fn notify_flag_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_program: String,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub branch_prefix: Option<String>,
    pub worktree_root: PathBuf,
    #[serde(default = "default_repos_root")]
    pub repos_root: PathBuf,
    pub tmux_session_prefix: String,
    pub poll_interval_ms: u64,
    pub dropr_overlay: bool,
    #[serde(default)]
    pub auto_accept: bool,
    #[serde(default = "notify_flag_default")]
    pub process_indicator: bool,
    #[serde(default = "notify_flag_default")]
    pub subagent_indicator: bool,
    /// The strategy every merge path passes to `gh pr merge` — the TUI's `m`
    /// key and the Overseer's auto-merge gate both read this one field. See
    /// [`merge_strategy::resolve`] for how a config still carrying the retired
    /// `overseer.merge_strategy` is migrated onto it.
    #[serde(default)]
    pub merge_strategy: MergeStrategy,
    /// How `merge_strategy` was arrived at, when that is worth telling the
    /// operator: a legacy `overseer.merge_strategy` adopted, or overruling a
    /// top-level value that disagreed with it. Derived on load and never part
    /// of the file.
    #[serde(skip)]
    pub merge_strategy_notice: Option<String>,
    /// Set when the loaded file's `overseer.discord` still carries a
    /// retired per-event `notify_*` boolean — see [`discord_notify`].
    /// Derived on load and never part of the file.
    #[serde(skip)]
    pub discord_legacy_notify_notice: Option<String>,
    /// Prompt sent to an agent when requesting a PR. Defaults to "Commit any
    /// remaining changes, push the branch, and open a pull request against main
    /// following the project's PR conventions."
    #[serde(default = "default_pr_prompt")]
    pub pr_prompt: String,
    /// Language every LLM surface is told to write its human-readable prose in,
    /// named the way you would say it to a person ("Japanese", "日本語",
    /// "Brazilian Portuguese"). `None` sends the prompts robco has always sent.
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub notify: NotifyConfig,
    #[serde(default)]
    pub openclaw: OpenClawConfig,
    #[serde(default)]
    pub project_icon: ProjectIcon,
    #[serde(default, alias = "chief")]
    pub overseer: OverseerConfig,
}

/// Presence probe for the top-level `merge_strategy`. [`Config`] deserializes
/// that field with a default, which cannot tell a key written as the default
/// from a key that is absent — and the legacy-key migration turns on exactly
/// that distinction.
#[derive(Deserialize)]
struct StrategyProbe {
    #[serde(default)]
    merge_strategy: Option<MergeStrategy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub autonomous_args: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    /// Slash command this profile's program understands as "wipe the chat
    /// history and start over" — Claude's `/clear`, or whatever the profile's
    /// own `program` calls the same action. `None` means the profile never
    /// declared one, so the `C` key on a repo row refuses rather than send a
    /// guessed string (dropr:550).
    #[serde(default)]
    pub clear_command: Option<String>,
}

pub(crate) fn default_profiles() -> Vec<Profile> {
    vec![
        Profile {
            name: "claude".to_string(),
            program: "claude".to_string(),
            autonomous_args: vec!["--dangerously-skip-permissions".to_string()],
            model: None,
            backend: None,
            clear_command: Some("/clear".to_string()),
        },
        Profile {
            name: "codex".to_string(),
            program: "codex".to_string(),
            autonomous_args: vec!["--dangerously-bypass-approvals-and-sandbox".to_string()],
            model: None,
            backend: None,
            // Confirmed against the installed `codex` CLI (v0.144.1): typing
            // `/clear` in its TUI resets the session back to the welcome
            // banner, the same effect Claude's `/clear` has.
            clear_command: Some("/clear".to_string()),
        },
    ]
}

fn default_repos_root() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".robco")
        .join("repos")
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
            repos_root: default_repos_root(),
            tmux_session_prefix: "robco_".to_string(),
            poll_interval_ms: 750,
            dropr_overlay: true,
            auto_accept: false,
            process_indicator: true,
            subagent_indicator: true,
            merge_strategy: MergeStrategy::default(),
            merge_strategy_notice: None,
            discord_legacy_notify_notice: None,
            pr_prompt: default_pr_prompt(),
            language: None,
            notify: NotifyConfig::default(),
            openclaw: OpenClawConfig::default(),
            project_icon: ProjectIcon::default(),
            overseer: OverseerConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_at(&config_path()?)
    }

    pub(crate) fn load_at(path: &Path) -> Result<Self> {
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
        config.repos_root = expand_tilde(&config.repos_root);
        config.migrate_merge_strategy(&raw)?;
        config.discord_legacy_notify_notice = discord_notify::detect_legacy_keys(&raw);
        Ok(config)
    }

    /// Collapses the retired `overseer.merge_strategy` onto the top-level
    /// `merge_strategy`, so the TUI and the Overseer cannot end a load holding
    /// different answers. The legacy key is cleared here and skipped on
    /// serialization, so the next save drops it from the file.
    fn migrate_merge_strategy(&mut self, raw: &str) -> Result<()> {
        let probe: StrategyProbe = serde_json::from_str(raw)?;
        let legacy = self.overseer.legacy_merge_strategy.take();
        let resolved = merge_strategy::resolve(probe.merge_strategy, legacy.as_deref());
        self.merge_strategy = resolved.strategy;
        self.merge_strategy_notice = resolved.notice;
        Ok(())
    }

    /// Atomically persist the config via a temp file and rename, so a crash
    /// mid-write leaves the previous config intact. Writes remain last-writer-wins:
    /// rare, human-driven config edits do not warrant a durable owner queue yet,
    /// so a writer holding a stale snapshot still reverts a concurrent edit. Every
    /// writer therefore reloads immediately before mutating — see
    /// `overseer::command::settings` and `overseer::config_write`.
    pub fn save(&self) -> Result<()> {
        ensure_robco_dir()?;
        self.save_at(&config_path()?)
    }

    pub(crate) fn save_at(&self, path: &Path) -> Result<()> {
        let raw = serde_json::to_string_pretty(self)?;
        let temp_path = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let written = fs::write(&temp_path, raw).and_then(|()| fs::rename(&temp_path, path));
        if let Err(error) = written {
            let _ = fs::remove_file(temp_path);
            return Err(error.into());
        }
        Ok(())
    }

    pub fn default_program_command(&self) -> String {
        self.profiles
            .iter()
            .find(|profile| profile.name == self.default_program)
            .map(|profile| profile.program.clone())
            .unwrap_or_else(|| self.default_program.clone())
    }

    pub fn default_program_autonomous_args(&self) -> Vec<String> {
        self.profiles
            .iter()
            .find(|profile| profile.name == self.default_program)
            .map(|profile| profile.autonomous_args.clone())
            .unwrap_or_default()
    }

    /// The clear command configured for the profile a repo's main-worktree
    /// session launches with, or `None` when that profile has not set one —
    /// see `Profile::clear_command`.
    pub fn default_program_clear_command(&self) -> Option<String> {
        self.profiles
            .iter()
            .find(|profile| profile.name == self.default_program)
            .and_then(|profile| profile.clear_command.clone())
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
