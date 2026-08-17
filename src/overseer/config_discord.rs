use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::NotifyLevel;

fn default_discord_token_env() -> String {
    "ROBCO_DISCORD_TOKEN".to_string()
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
    /// Binds a chat channel id to one repository name, so the ops agent's
    /// briefing can scope its ledger rows, decisions, and tasks to that repo
    /// instead of every repo the daemon manages (dropr:450). An operator
    /// edits this map directly, the same way every other Discord setting
    /// here is set — there is no dedicated command, deliberately: binding is
    /// routing configuration, not an action the closed `Command` enum should
    /// grow a variant for. A channel id absent from this map is unbound and
    /// keeps today's unscoped behaviour.
    pub channel_repo_bindings: BTreeMap<String, String>,
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
            channel_repo_bindings: BTreeMap::new(),
        }
    }
}
