use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use super::autonomy::AutonomyLevel;

/// How strictly the auto-merge gate requires the base branch to be protected.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "kebab-case")]
pub enum ProtectionMode {
    /// Require both a pull-request requirement and at least one required status check.
    #[default]
    Required,
    /// Require only that changes go through a pull request.
    Relaxed,
    /// Skip the protection probe and rely on GitHub's mergeability signal alone.
    Off,
}

impl ProtectionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Relaxed => "relaxed",
            Self::Off => "off",
        }
    }
}

fn default_worker_blocklist() -> Vec<String> {
    ["AWS_*", "*_TOKEN", "*_SECRET", "*_API_KEY"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn default_discord_token_env() -> String {
    "ROBCO_DISCORD_TOKEN".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OverseerConfig {
    pub enabled: bool,
    pub dispatch_enabled: bool,
    pub auto_merge: bool,
    pub protection_mode: ProtectionMode,
    pub autonomy_level: AutonomyLevel,
    pub daily_llm_budget: u32,
    pub merge_strategy: String,
    pub max_branch_updates: u32,
    pub worker_profile: Option<String>,
    pub max_workers: usize,
    pub per_repo_limit: usize,
    pub poll_interval_secs: u64,
    pub stuck_after_mins: u64,
    pub max_retries_per_task: u32,
    pub daily_dispatch_limit: u32,
    pub failure_circuit_threshold: u32,
    pub triage_enabled: bool,
    pub triage_profile: Option<String>,
    pub judge_profile: Option<String>,
    pub merge_judge_profile: Option<String>,
    pub triage_timeout_mins: u64,
    pub worker_env_blocklist: Vec<String>,
    pub dispatch_task_authors: Vec<String>,
    pub discord: DiscordConfig,
}

impl Default for OverseerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dispatch_enabled: true,
            auto_merge: false,
            protection_mode: ProtectionMode::Required,
            autonomy_level: AutonomyLevel::Conservative,
            daily_llm_budget: 200,
            merge_strategy: "squash".into(),
            max_branch_updates: 3,
            worker_profile: None,
            max_workers: 3,
            per_repo_limit: 1,
            poll_interval_secs: 60,
            stuck_after_mins: 30,
            max_retries_per_task: 1,
            daily_dispatch_limit: 20,
            failure_circuit_threshold: 3,
            triage_enabled: true,
            triage_profile: None,
            judge_profile: None,
            merge_judge_profile: None,
            triage_timeout_mins: 15,
            worker_env_blocklist: default_worker_blocklist(),
            dispatch_task_authors: Vec::new(),
            discord: DiscordConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub token_env: String,
    pub channel_id: Option<String>,
    pub allowed_user_ids: Vec<String>,
    pub notify_escalation: bool,
    pub notify_pr_opened: bool,
    pub notify_merged: bool,
    pub notify_circuit: bool,
    pub notify_worker_blocked: bool,
    pub action_limit_per_hour: usize,
    pub confirmation_ttl_secs: u64,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token_env: default_discord_token_env(),
            channel_id: None,
            allowed_user_ids: Vec::new(),
            notify_escalation: true,
            notify_pr_opened: true,
            notify_merged: true,
            notify_circuit: true,
            notify_worker_blocked: true,
            action_limit_per_hour: 30,
            confirmation_ttl_secs: 120,
        }
    }
}
