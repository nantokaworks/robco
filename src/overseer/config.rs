use std::{collections::BTreeMap, path::PathBuf};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use super::autonomy::AutonomyLevel;

#[path = "config_notify_level.rs"]
mod notify_level;
pub use notify_level::{NotifyLevel, NotifyTier};

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

/// Matches `max_branch_updates`: both bound how many times one entry may spend a
/// resource on catching up to a base that keeps moving, and a pull request that
/// needs more looks than this is one an operator should see anyway.
fn default_max_merge_judge_primes() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OverseerConfig {
    pub dispatch_enabled: bool,
    pub auto_merge: bool,
    pub protection_mode: ProtectionMode,
    /// Lets the auto-merge gate proceed on a repository whose GitHub plan does not
    /// expose branch protection at all (every probe answers `403`), rather than
    /// holding it under `unprotected:plan_unsupported` forever. Off by default: this
    /// is a security-relevant relaxation, not a convenience knob — enabling it means
    /// Overseer will merge onto a base branch it could never confirm is protected,
    /// indistinguishable from one whose owner simply never configured protection at
    /// all, because GitHub's plan-limited 403 does not say which. It does not affect
    /// any repository whose plan can actually answer the probe: those keep being
    /// held on their real facts exactly as before. Global rather than per-repository
    /// because the condition it targets — a probe that cannot possibly answer — only
    /// ever arises on a plan-limited repository in the first place.
    pub allow_unverifiable_protection: bool,
    pub autonomy_level: AutonomyLevel,
    pub daily_llm_budget: u32,
    /// Retired: the merge strategy is the top-level `merge_strategy`, which the
    /// TUI reads too. Kept as a read-only field so a config still carrying the
    /// key parses and `Config::load_at` can migrate its value; it is skipped on
    /// serialization, so the next save drops the key rather than leaving two
    /// settings that can drift apart again.
    #[serde(rename = "merge_strategy", skip_serializing)]
    pub legacy_merge_strategy: Option<String>,
    /// See `ledger::LedgerEntry::branch_updates` for what this still bounds.
    pub max_branch_updates: u32,
    /// See `ledger::LedgerEntry::merge_judge_primes`. `0` turns early judgments
    /// off entirely, leaving every merge judgment to run after the gate clears.
    #[serde(default = "default_max_merge_judge_primes")]
    pub max_merge_judge_primes: u32,
    /// Auto-merge passes one repository may be held waiting for its post-merge
    /// `git pull --ff-only` before the barrier is lifted without it. The bound is
    /// what stops a pull that never succeeds from parking the repository
    /// forever. At the default poll interval this is five minutes.
    pub max_merge_settle_passes: u32,
    /// Whether a merge failure the owning worker could fix is handed back to it
    /// instead of parking the pull request. Default-off, so a daemon that has
    /// never heard of merge recovery behaves exactly as it did before it existed.
    pub merge_recovery_enabled: bool,
    /// Handbacks one pull request may be charged before it escalates to a human.
    /// The ceiling is what stops a worker that cannot fix the failure from being
    /// re-prompted on every push.
    pub max_merge_recoveries: u32,
    /// Auto-merge passes one pull request may be held under the same reason at
    /// the same head before it escalates. Without a bound, every gate exit that
    /// is not a merge re-records its reason once per poll for as long as the
    /// condition lasts, which is how a pull request came to be held for seven
    /// hours with nothing on the board saying so.
    ///
    /// At the default poll interval the default is thirty minutes: comfortably
    /// past the 5-15 minutes a healthy check run takes, and still well inside an
    /// hour. `0` escalates on the first held pass.
    pub max_merge_holds: u32,
    /// Fail-safe merge verdicts (the judge session itself failed rather than
    /// saying anything about the change) one pull request may receive before it
    /// escalates for real. A fail-safe verdict is not cached as a terminal
    /// judgment, so the next pass re-asks the judge — but a permanently broken
    /// judge must still converge instead of spending model budget in a loop,
    /// which is what this bounds. `0` escalates on the first fail-safe verdict.
    pub max_merge_judge_fail_safes: u32,
    /// Reconsiderations one entry the hold cap escalated may be given, each a
    /// full pass back through the gate to check whether the condition it held
    /// on has cleared — an operator flipping a setting, or a probe that
    /// answers now instead of timing out. Unlike `max_merge_holds`, this is not
    /// keyed on the reason staying the same: it exists precisely because a
    /// pre-judge condition (protection, checks, merge state) is never cached
    /// anywhere the gate can watch for a change on its own, so it must be
    /// re-tried instead. Bounded so a condition that never clears still stops
    /// being reconsidered rather than polling forever. `0` never reconsiders a
    /// hold-cap escalation.
    pub max_merge_hold_rechecks: u32,
    pub worker_profile: Option<String>,
    pub max_workers: usize,
    pub per_repo_limit: usize,
    /// Settled ledger entries kept per repository. Nothing else ever removes a
    /// terminal entry, so without a bound the ledger grows for the life of the
    /// installation and every save rewrites all of it. `0` keeps every entry,
    /// which is exactly how the ledger behaved before the window existed. See
    /// `crate::overseer::daemon::retention` for what the window refuses to
    /// drop.
    pub terminal_retention_per_repo: usize,
    pub poll_interval_secs: u64,
    pub stuck_after_mins: u64,
    pub max_retries_per_task: u32,
    pub daily_dispatch_limit: u32,
    pub failure_circuit_threshold: u32,
    pub triage_enabled: bool,
    pub triage_profile: Option<String>,
    pub judge_profile: Option<String>,
    pub merge_judge_profile: Option<String>,
    pub max_concurrent_judges: usize,
    /// Profile the periodic board reviewer runs under. `None` disables the
    /// review pass outright — it is the whole on/off switch, so a daemon that
    /// has never heard of the reviewer behaves exactly as it did before it
    /// existed.
    pub review_profile: Option<String>,
    pub review_interval_mins: u64,
    /// Daily call budget for the board reviewer, deliberately separate from
    /// `daily_llm_budget`. The reviewer runs on a clock rather than on demand,
    /// so sharing one budget would let it starve dispatch and merge judgement.
    pub daily_review_budget: u32,
    pub triage_timeout_mins: u64,
    pub worker_env_blocklist: Vec<String>,
    /// Environment handed to every session the daemon spawns, and the highest
    /// layer of the credential channel documented in
    /// `crate::overseer::session::env`. A launchd agent has no access to the
    /// interactive login session's keychain, so this — or the env file below —
    /// is how a `claude setup-token` credential reaches a judge, triage, or
    /// review session at all.
    ///
    /// Names set here are exempt from `worker_env_blocklist`: the blocklist
    /// blanks *ambient* names a worker never asked for, while an entry here is
    /// an operator naming what the sessions need.
    pub session_env: BTreeMap<String, String>,
    /// `KEY=VALUE` file read below `session_env` and above the daemon's own
    /// environment. `None` reads `~/.robco/env`. Kept separate from the config
    /// so a rotated token is one file write rather than a config edit, and so
    /// the secret can live in a file with its own permissions.
    pub session_env_file: Option<PathBuf>,
    /// Whether the daemon spawns one probe session at start-up to confirm the
    /// credential channel actually authenticates. Default-on: the failure it
    /// catches is otherwise only visible as merge judgments escalating.
    pub session_preflight: bool,
    pub dispatch_task_authors: Vec<String>,
    /// Hours a ledger entry may sit waiting on a dropr `blocks` dependency
    /// edge — a worker's `waiting-prerequisite` report, or the auto-merge
    /// gate's own hold on a pull request whose task carries one — before it
    /// escalates to an operator. Ordering waits are expected to run far
    /// longer than a CI check or a review (`max_merge_holds`'s scale), since
    /// what they wait on is another task's own implementation and review
    /// cycle, not a few minutes of automation; the default gives that
    /// several working days before treating the wait as a stuck or cyclical
    /// dependency instead of the steady state it is meant to be. See
    /// `overseer::ledger::LedgerEntry::prerequisite_wait`.
    pub max_prerequisite_wait_hours: u64,
    /// Whether a merge that closes a `[release]`-scoped task in this
    /// project's own repository runs `scripts/release.sh` unattended — see
    /// `overseer::release_pipeline`. This is a distinct privilege class from
    /// every other flag in this file: every other toggle here reads or acts
    /// on GitHub through `gh`, but this one runs a local shell script for up
    /// to thirty minutes and, on success, publishes a public GitHub release
    /// using whatever credentials the daemon holds. Default-off, matching
    /// `merge_recovery_enabled`'s precedent for a capability that widens
    /// what the daemon does unattended: an operator opts in deliberately,
    /// after weighing that `scripts/release.sh` is itself part of this
    /// repository and a future change to it would run with this same
    /// privilege the next time a `[release]`-scoped merge lands.
    pub release_pipeline_enabled: bool,
    pub discord: DiscordConfig,
}

impl Default for OverseerConfig {
    fn default() -> Self {
        Self {
            dispatch_enabled: true,
            auto_merge: false,
            protection_mode: ProtectionMode::Required,
            allow_unverifiable_protection: false,
            autonomy_level: AutonomyLevel::Conservative,
            daily_llm_budget: 200,
            legacy_merge_strategy: None,
            max_branch_updates: 3,
            max_merge_judge_primes: default_max_merge_judge_primes(),
            max_merge_settle_passes: 5,
            merge_recovery_enabled: false,
            max_merge_recoveries: 2,
            max_merge_holds: 30,
            max_merge_judge_fail_safes: 3,
            max_merge_hold_rechecks: 10,
            worker_profile: None,
            max_workers: 3,
            per_repo_limit: 1,
            // Deep enough that a repository's recent history is still readable
            // — well past the live-entry cap every other surface applies — and
            // shallow enough that the file the daemon rewrites every pass stays
            // small.
            terminal_retention_per_repo: 50,
            poll_interval_secs: 60,
            stuck_after_mins: 30,
            max_retries_per_task: 1,
            daily_dispatch_limit: 20,
            failure_circuit_threshold: 3,
            triage_enabled: true,
            triage_profile: None,
            judge_profile: None,
            merge_judge_profile: None,
            max_concurrent_judges: 4,
            review_profile: None,
            review_interval_mins: 20,
            // 96 covers a full day at the shortest sensible cadence (15 min),
            // so shortening the interval does not silently truncate the day.
            daily_review_budget: 96,
            triage_timeout_mins: 15,
            worker_env_blocklist: default_worker_blocklist(),
            session_env: BTreeMap::new(),
            session_env_file: None,
            session_preflight: true,
            dispatch_task_authors: Vec::new(),
            // 72 hours: several working days, comfortably longer than any
            // CI-scale budget in this file, and still a bound rather than
            // "forever" — see the field's own doc for why.
            max_prerequisite_wait_hours: 72,
            release_pipeline_enabled: false,
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
    /// Channel that receives the Overseer's reports — decision notifications
    /// and digests. Unset, reports fall back to `channel_id`, so a config
    /// written before this field existed keeps its single-channel behavior.
    /// Chat replies, command responses, and confirmations never route
    /// through this; they stay on the channel the message came from.
    pub notify_channel_id: Option<String>,
    pub allowed_user_ids: Vec<String>,
    /// Notification verbosity baseline; the sole gate for which events post.
    pub notify_level: NotifyLevel,
    /// Runs notification titles and descriptions through an LLM pass in
    /// `language` before posting. Independent of `language` itself, which
    /// also governs the ops-agent reply and every worker/judge prompt — an
    /// operator may want those localized but keep templated notifications
    /// in English for speed and cost. Has no effect while `language` is
    /// unset or blank; the pass is skipped either way.
    pub notify_localize: bool,
    /// Discord category IDs whose text channels get a conversational reply
    /// to plain chat, the same way `channel_id` already does. Empty by
    /// default, so the feature is off until an operator opts in — an
    /// operator may want every message under that category answered without
    /// naming each channel, e.g. as channels are added or renamed.
    pub chat_category_ids: Vec<String>,
    /// Concurrent ops-agent sessions across `channel_id` and every
    /// `chat_category_ids` channel combined. Each session is a spawned OS
    /// thread running an agent CLI, so this is a real resource bound, not
    /// just a Discord-noise knob — see `OpsAgent`'s per-channel session map.
    /// Beyond the cap a channel gets the same busy reply a single-slot agent
    /// already returned, rather than a dropped message.
    pub chat_concurrency_cap: usize,
    pub action_limit_per_hour: usize,
    pub confirmation_ttl_secs: u64,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token_env: default_discord_token_env(),
            channel_id: None,
            notify_channel_id: None,
            allowed_user_ids: Vec::new(),
            notify_level: NotifyLevel::default(),
            notify_localize: true,
            chat_category_ids: Vec::new(),
            chat_concurrency_cap: 3,
            action_limit_per_hour: 30,
            confirmation_ttl_secs: 120,
        }
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
