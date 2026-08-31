use std::{collections::HashSet, time::SystemTime};

use chrono::{DateTime, Local};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    agent,
    config::Config,
    discover, git,
    model::{RepoNode, Status},
    overseer::discord_channels::DiscordChannels,
    registry::Registry,
    status::{self, WatchStatusState},
    subagents::{SubagentReader, SubagentStatus, claude::ClaudeSubagentReader, read_allowed},
    tmux,
};

use super::{ToolResult, exec_err};

#[path = "discovery_paths.rs"]
mod paths;
use paths::{is_managed_worktree, matches_slot, path_is_strictly_inside, path_key};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DiscoverySnapshotArgs {}

pub(super) fn snapshot(_args: DiscoverySnapshotArgs) -> ToolResult<Value> {
    let config = Config::load().map_err(exec_err)?;
    let mut registry = Registry::load().map_err(exec_err)?;
    let discovered = discover::discover_all([config.repos_root.as_path()]);
    registry.merge_discovered(discovered);
    for repo in &mut registry.repos {
        status::refresh_main_drift(repo);
        status::refresh_checkout_branch(repo);
    }
    let panes = tmux::capture_panes(&config.tmux_server).ok();
    let reader = ClaudeSubagentReader::default();
    let now = SystemTime::now();
    let registry_value = registry_json(&registry, &config, panes.as_ref(), &reader, now)?;
    let discord_channels = crate::overseer::discord_ops_dir()
        .ok()
        .and_then(|dir| DiscordChannels::load(&dir.join("channels.json")).ok())
        .unwrap_or_default();
    let orphans = discover_orphans(&config, &registry.repos, &discord_channels);
    Ok(json!({
        "registry": registry_value,
        "orphans": orphans,
    }))
}

fn registry_json(
    registry: &Registry,
    config: &Config,
    panes: Option<&tmux::PaneSnapshot>,
    reader: &dyn SubagentReader,
    now: SystemTime,
) -> ToolResult<Value> {
    let mut value = serde_json::to_value(registry).map_err(exec_err)?;
    let repos = value["repos"]
        .as_array_mut()
        .expect("serialized registry repos must be an array");
    for (repo_value, repo) in repos.iter_mut().zip(&registry.repos) {
        let repo_object = repo_value
            .as_object_mut()
            .expect("serialized repository must be an object");
        let main_session = agent::repo_claude_session_name(&config.tmux_session_prefix, repo);
        let main_status =
            status::classify_session_status(&main_session, None, &mut WatchStatusState::default());
        insert(repo_object, "main_status", main_status.map(Status::badge));
        insert(repo_object, "main_behind_origin", repo.main_behind_origin);
        insert(
            repo_object,
            "checkout_state",
            repo.checkout_state.as_ref().map(checkout_state_json),
        );
        let main_count = if config.subagent_indicator && main_status.is_some() {
            running_count(&reader.read(&repo.path, None, now))
        } else {
            0
        };
        insert(repo_object, "main_subagents_active", main_count);
        let children = child_worktrees(repo, config);
        let agents = repo_object["agents"]
            .as_array_mut()
            .expect("serialized agents must be an array");
        for ((agent_value, agent), children) in agents.iter_mut().zip(&repo.agents).zip(children) {
            let object = agent_value
                .as_object_mut()
                .expect("serialized agent must be an object");
            let report = status::classify_agent_status(
                &repo.path,
                &agent.worktree_path,
                &agent.branch,
                &agent.tmux_session,
                &mut WatchStatusState::default(),
                panes,
            );
            let agent_status = report.map_or(Status::Dead, |report| report.status);
            insert(object, "status", agent_status.badge());
            insert(
                object,
                "worktree_missing",
                report.is_some_and(|r| r.worktree_missing),
            );
            let count =
                if config.subagent_indicator && read_allowed(agent_status, &agent.worktree_path) {
                    running_count(&reader.read(
                        &agent.worktree_path,
                        agent.claude_session_id.as_deref(),
                        now,
                    ))
                } else {
                    0
                };
            insert(object, "subagents_active", count);
            insert(object, "children", children);
        }
    }
    Ok(value)
}

fn checkout_state_json(state: &crate::model::CheckoutState) -> Value {
    match state {
        crate::model::CheckoutState::Detached { default_branch } => {
            json!({ "kind": "detached", "default_branch": default_branch })
        }
        crate::model::CheckoutState::OtherBranch {
            current,
            default_branch,
        } => json!({
            "kind": "other_branch",
            "current": current,
            "default_branch": default_branch,
        }),
        crate::model::CheckoutState::DefaultBranchUnknown => {
            json!({ "kind": "default_branch_unknown" })
        }
    }
}

fn insert(object: &mut Map<String, Value>, key: &str, value: impl serde::Serialize) {
    object.insert(key.into(), serde_json::to_value(value).unwrap());
}

fn running_count(subagents: &[crate::subagents::TaskSubagent]) -> usize {
    subagents
        .iter()
        .filter(|subagent| subagent.status == SubagentStatus::Running)
        .count()
}

fn child_worktrees(repo: &RepoNode, config: &Config) -> Vec<Vec<Value>> {
    let mut children = vec![Vec::new(); repo.agents.len()];
    let Ok(worktrees) = git::list_worktrees(&repo.path) else {
        return children;
    };
    for worktree in worktrees {
        if path_key(&worktree.path) == path_key(&repo.path)
            || repo
                .agents
                .iter()
                .any(|agent| path_key(&agent.worktree_path) == path_key(&worktree.path))
        {
            continue;
        }
        let owner = repo.agents.iter().position(|agent| {
            path_is_strictly_inside(&worktree.path, &agent.worktree_path)
                || matches_slot(&worktree.path, worktree.branch.as_deref(), agent)
        });
        let Some(owner) = owner else { continue };
        let agent = &repo.agents[owner];
        let ahead_behind = worktree
            .branch
            .as_deref()
            .and_then(|branch| git::ahead_behind(&repo.path, &agent.branch, branch).ok());
        let modified_at = std::fs::metadata(&worktree.path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .map(DateTime::<Local>::from);
        children[owner].push(json!({
            "path": worktree.path,
            "branch": worktree.branch,
            "head": worktree.head,
            "clean": git::tracked_tree_is_clean(&worktree.path).ok(),
            "ahead_behind": ahead_behind,
            "tmux_session": tmux::find_session_by_cwd(
                &config.tmux_server,
                &config.tmux_session_prefix,
                &worktree.path,
            ),
            "modified_at": modified_at,
        }));
    }
    children
}

fn discover_orphans(
    config: &Config,
    repos: &[RepoNode],
    discord_channels: &DiscordChannels,
) -> Option<Vec<Value>> {
    let sessions = tmux::list_sessions_with_cwd(&config.tmux_server).ok()?;
    let mut known = HashSet::from([crate::overseer::control_session_name(
        &config.tmux_session_prefix,
    )]);
    for channel_id in discord_channels.channels.keys() {
        known.insert(crate::overseer::discord_channel_session_name(
            &config.tmux_session_prefix,
            channel_id,
        ));
    }
    for repo in repos {
        known.insert(agent::repo_claude_session_name(
            &config.tmux_session_prefix,
            repo,
        ));
        known.insert(agent::repo_shell_session_name(
            &config.tmux_session_prefix,
            repo,
        ));
        for tracked in &repo.agents {
            known.insert(tracked.tmux_session.clone());
            known.insert(agent::shell_session_name(tracked));
        }
        for worktree in git::list_worktrees(&repo.path).unwrap_or_default() {
            if let Some(session) = tmux::find_session_by_cwd(
                &config.tmux_server,
                &config.tmux_session_prefix,
                &worktree.path,
            ) {
                known.insert(session);
            }
        }
    }
    let mut orphans = sessions
        .into_iter()
        .filter(|(name, cwd)| {
            name.starts_with(&config.tmux_session_prefix)
                && !known.contains(name)
                && is_managed_worktree(cwd, &config.worktree_root)
        })
        .map(|(name, cwd)| json!({ "name": name, "cwd": cwd }))
        .collect::<Vec<_>>();
    orphans.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Some(orphans)
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
