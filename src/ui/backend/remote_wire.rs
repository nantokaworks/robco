use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

use chrono::{DateTime, Local};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    model::{CheckoutState, ChildWorktree, OrphanSession, Status},
    overseer::{
        config::OverseerConfig, discord_channels::DiscordChannels, dismissals::Dismissals,
        ledger::Ledger, logging::DecisionEntry, other_prs::OtherPrs, row_summaries::RowSummaries,
    },
    registry::Registry,
    subagents::{SubagentStatus, TaskSubagent},
};

#[derive(Deserialize)]
pub(super) struct DiscoveryWire {
    registry: Value,
    orphans: Option<Vec<OrphanWire>>,
}

#[derive(Deserialize)]
struct OrphanWire {
    name: String,
    cwd: PathBuf,
}

impl DiscoveryWire {
    pub(super) fn into_parts(self) -> serde_json::Result<(Registry, Option<Vec<OrphanSession>>)> {
        let mut registry: Registry = serde_json::from_value(self.registry.clone())?;
        hydrate_registry(&mut registry, &self.registry);
        let orphans = self.orphans.map(|items| {
            items
                .into_iter()
                .map(|item| OrphanSession {
                    name: item.name,
                    cwd: item.cwd,
                })
                .collect()
        });
        Ok((registry, orphans))
    }
}

#[derive(Deserialize)]
pub(super) struct OverseerWire {
    pub overseer: OverseerConfig,
    pub ledger: Ledger,
    pub other_prs: OtherPrs,
    pub discord_channels: DiscordChannels,
    pub decisions: Vec<DecisionEntry>,
    pub dismissals: Dismissals,
    pub row_summaries: RowSummaries,
    pub daemon_pid_alive: bool,
    pub daemon_alive: bool,
    pub heartbeat_age: Option<Duration>,
    pub daemon_version: Option<String>,
    pub control_status: Option<String>,
}

pub(super) fn status(value: &str) -> Option<Status> {
    Some(match value {
        "idle" => Status::Idle,
        "run" => Status::Running,
        "wait" => Status::Waiting,
        "done" => Status::Done,
        "dead" => Status::Dead,
        "branch" => Status::BranchOnly,
        _ => return None,
    })
}

fn hydrate_registry(registry: &mut Registry, raw: &Value) {
    let Some(raw_repos) = raw.get("repos").and_then(Value::as_array) else {
        return;
    };
    for (repo, raw_repo) in registry.repos.iter_mut().zip(raw_repos) {
        repo.main_status = raw_repo
            .get("main_status")
            .and_then(Value::as_str)
            .and_then(status);
        repo.main_behind_origin = raw_repo
            .get("main_behind_origin")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok());
        repo.main_subagents_active = usize_field(raw_repo, "main_subagents_active");
        repo.checkout_state = raw_repo.get("checkout_state").and_then(checkout_state);
        let Some(raw_agents) = raw_repo.get("agents").and_then(Value::as_array) else {
            continue;
        };
        for (agent, raw_agent) in repo.agents.iter_mut().zip(raw_agents) {
            agent.status = raw_agent
                .get("status")
                .and_then(Value::as_str)
                .and_then(status)
                .unwrap_or(Status::Dead);
            agent.worktree_missing = raw_agent
                .get("worktree_missing")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            agent.tracked_command = raw_agent
                .get("tracked_command")
                .and_then(Value::as_str)
                .map(str::to_owned);
            agent.subagents = (0..usize_field(raw_agent, "subagents_active"))
                .map(|index| TaskSubagent {
                    id: format!("remote-{index}"),
                    agent_type: "remote".into(),
                    description: String::new(),
                    spawn_depth: 0,
                    started_at: SystemTime::UNIX_EPOCH,
                    last_activity_at: SystemTime::UNIX_EPOCH,
                    status: SubagentStatus::Running,
                })
                .collect();
            agent.children = raw_agent
                .get("children")
                .and_then(Value::as_array)
                .map(|children| children.iter().filter_map(child).collect())
                .unwrap_or_default();
        }
    }
}

fn usize_field(value: &Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0)
}

fn checkout_state(value: &Value) -> Option<CheckoutState> {
    let kind = value.get("kind")?.as_str()?;
    let default_branch = || value.get("default_branch")?.as_str().map(str::to_owned);
    match kind {
        "detached" => Some(CheckoutState::Detached {
            default_branch: default_branch()?,
        }),
        "other_branch" => Some(CheckoutState::OtherBranch {
            current: value.get("current")?.as_str()?.to_owned(),
            default_branch: default_branch()?,
        }),
        "default_branch_unknown" => Some(CheckoutState::DefaultBranchUnknown),
        _ => None,
    }
}

fn child(value: &Value) -> Option<ChildWorktree> {
    Some(ChildWorktree {
        path: serde_json::from_value(value.get("path")?.clone()).ok()?,
        branch: optional(value, "branch"),
        head: optional(value, "head"),
        clean: value.get("clean").and_then(Value::as_bool),
        ahead_behind: value.get("ahead_behind").and_then(|pair| {
            let pair = pair.as_array()?;
            Some((
                u32::try_from(pair.first()?.as_u64()?).ok()?,
                u32::try_from(pair.get(1)?.as_u64()?).ok()?,
            ))
        }),
        tmux_session: optional(value, "tmux_session"),
        modified_at: value
            .get("modified_at")
            .cloned()
            .and_then(|v| serde_json::from_value::<DateTime<Local>>(v).ok()),
    })
}

fn optional(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}
