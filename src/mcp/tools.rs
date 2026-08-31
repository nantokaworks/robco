use std::time::SystemTime;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    model::{AgentNode, RepoNode, Status},
    overseer::discord::{commands::Command, mcp_bridge},
    registry::Registry,
    status::{self, StatusReport, WatchStatusState},
    subagents::{SubagentReader, SubagentStatus, claude::ClaudeSubagentReader, read_allowed},
    tmux,
};

const PROMPT_LINES: usize = 20;

mod approve;
mod catalog;
mod discovery;
mod identity;
mod merge;
mod overseer_snapshot;
mod pane_capture;
mod policy;
mod pr;
mod questions;
mod report;
mod shared;
mod spawn;

pub use catalog::list_tools;
pub(crate) use report::{deliver_report, report_exit_code};
pub(crate) use shared::{agent_create, pr_request, pr_status, question_list, whoami};

#[derive(Debug)]
pub enum ToolError {
    InvalidParams(String),
    Execution(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidParams(message) | Self::Execution(message) => f.write_str(message),
        }
    }
}

pub type ToolResult<T> = std::result::Result<T, ToolError>;

pub fn call_tool(name: &str, arguments: Option<Value>) -> ToolResult<Value> {
    match name {
        "robco_whoami" => identity::whoami(),
        "robco_report" => {
            let args: report::ReportArgs = parse_args(arguments)?;
            report::report(args)
        }
        "robco_overseer_policy" | "robco_chief_policy" => {
            let args: policy::PolicyArgs = parse_args(arguments)?;
            policy::policy(args)
        }
        "robco_overseer_snapshot" => overseer_snapshot::snapshot(parse_args(arguments)?),
        "robco_pane_capture" => pane_capture::capture(parse_args(arguments)?),
        "robco_discovery_snapshot" => discovery::snapshot(parse_args(arguments)?),
        "robco_agent_list" => {
            let registry = Registry::load().map_err(exec_err)?;
            agent_list(&registry)
        }
        "robco_agent_create" => {
            let args: spawn::SpawnArgs = parse_args(arguments)?;
            spawn::spawn(args)
        }
        "robco_agent_status" => {
            let args: AgentIdArgs = parse_args(arguments)?;
            validate_non_blank("agent_id", &args.agent_id)?;
            let registry = Registry::load().map_err(exec_err)?;
            agent_status(&registry, &args.agent_id)
        }
        "robco_question_list" => questions::list(),
        "robco_answer" => {
            let args: AnswerArgs = parse_args(arguments)?;
            validate_non_blank("agent_id", &args.agent_id)?;
            validate_non_blank("text", &args.text)?;
            let registry = Registry::load().map_err(exec_err)?;
            answer(&registry, &args.agent_id, &args.text)
        }
        "robco_approve" => {
            let args: approve::ApproveArgs = parse_args(arguments)?;
            validate_non_blank("agent_id", &args.agent_id)?;
            let registry = Registry::load().map_err(exec_err)?;
            approve::approve(&registry, &args)
        }
        "robco_pr_status" => pr::pr_status(parse_args(arguments)?),
        "robco_pr_request" => pr::pr_request(parse_args(arguments)?),
        "robco_merge" => merge::merge(parse_args(arguments)?),
        "robco_tasks" => mcp_bridge::text_result(&Command::Tasks),
        "robco_log" => {
            let args: LogArgs = parse_args(arguments)?;
            mcp_bridge::text_result(&Command::Log(args.limit.unwrap_or(10).min(50)))
        }
        "robco_diff" => {
            let args: DiffArgs = parse_args(arguments)?;
            validate_non_blank("task_id", &args.task_id)?;
            mcp_bridge::text_result(&Command::Diff(args.task_id))
        }
        "robco_inbox" => mcp_bridge::text_result(&Command::Inbox),
        "robco_help" => mcp_bridge::text_result(&Command::Help),
        _ => Err(invalid_params(format!("unknown tool: {name}"))),
    }
}

fn agent_list(registry: &Registry) -> ToolResult<Value> {
    let repos = registry
        .repos
        .iter()
        .map(|repo| {
            let agents = repo
                .agents
                .iter()
                .map(|agent| {
                    let report = live_status(repo, agent);
                    let (tracked_command, subagents_active) = live_activity(agent, report.status);
                    json!({
                        "id": agent.id,
                        "title": agent.title,
                        "branch": agent.branch,
                        "tmux_session": agent.tmux_session,
                        "status": report.status.badge(),
                        "worktree_missing": report.worktree_missing,
                        "tracked_command": tracked_command,
                        "subagents_active": subagents_active
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "name": repo.name,
                "path": repo.path,
                "agents": agents
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "repos": repos }))
}

fn agent_status(registry: &Registry, agent_id: &str) -> ToolResult<Value> {
    let (repo, agent) = find_agent(registry, agent_id)?;
    let report = live_status(repo, agent);
    let (tracked_command, subagents_active) = live_activity(agent, report.status);
    Ok(json!({
        "agent_id": agent.id,
        "title": agent.title,
        "tmux_session": agent.tmux_session,
        "status": report.status.badge(),
        "worktree_missing": report.worktree_missing,
        "tracked_command": tracked_command,
        "subagents_active": subagents_active,
        "awaiting_confirmation": report.awaiting_confirmation,
        "prompt": prompt_tail(&agent.tmux_session)
    }))
}

fn live_activity(agent: &AgentNode, status: Status) -> (Option<String>, usize) {
    let subagents = if !read_allowed(status, &agent.worktree_path) {
        Vec::new()
    } else if agent.subagents.is_empty() {
        ClaudeSubagentReader::default().read(
            &agent.worktree_path,
            agent.claude_session_id.as_deref(),
            SystemTime::now(),
        )
    } else {
        agent.subagents.clone()
    };
    let subagents_active = subagents
        .iter()
        .filter(|subagent| subagent.status == SubagentStatus::Running)
        .count();
    let tracked_command = agent.tracked_command.clone().or_else(|| {
        let pane_pid = tmux::pane_pid(&tmux::TmuxServer::default_server(), &agent.tmux_session)
            .ok()
            .flatten()?;
        status::proc::ProcSnapshot::capture()
            .ok()?
            .tracked_command(pane_pid)
    });
    (tracked_command, subagents_active)
}

fn answer(registry: &Registry, agent_id: &str, text: &str) -> ToolResult<Value> {
    let (_, agent) = find_agent(registry, agent_id)?;
    let server = tmux::TmuxServer::default_server();
    tmux::send_literal_text(&server, &agent.tmux_session, text).map_err(exec_err)?;
    tmux::send_keys(&server, &agent.tmux_session, &["Enter"]).map_err(exec_err)?;
    Ok(json!({ "ok": true }))
}

pub(super) fn live_status(repo: &RepoNode, agent: &AgentNode) -> StatusReport {
    let mut state = WatchStatusState::default();
    let panes = tmux::capture_panes(&tmux::TmuxServer::default_server()).ok();
    status::classify_agent_status(
        &repo.path,
        &agent.worktree_path,
        &agent.branch,
        &agent.tmux_session,
        &mut state,
        panes.as_ref(),
    )
    .unwrap_or(StatusReport {
        status: Status::Dead,
        awaiting_confirmation: false,
        worktree_missing: false,
        mcp_active: false,
    })
}

pub(super) fn prompt_tail(session: &str) -> String {
    let server = tmux::TmuxServer::default_server();
    tmux::capture_plain(&server, session)
        .or_else(|_| tmux::capture_text(&server, session))
        .map(|capture| tail_non_empty_lines(&capture, PROMPT_LINES))
        .unwrap_or_default()
}

fn tail_non_empty_lines(text: &str, limit: usize) -> String {
    let mut lines = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(limit)
        .collect::<Vec<_>>();
    lines.reverse();
    lines.join("\n")
}

fn find_agent<'a>(
    registry: &'a Registry,
    agent_id: &str,
) -> ToolResult<(&'a RepoNode, &'a AgentNode)> {
    registry
        .repos
        .iter()
        .find_map(|repo| {
            repo.agents
                .iter()
                .find(|agent| agent.id == agent_id)
                .map(|agent| (repo, agent))
        })
        .ok_or_else(|| exec_err(format!("agent_id not found: {agent_id}")))
}

fn parse_args<T: for<'de> Deserialize<'de>>(arguments: Option<Value>) -> ToolResult<T> {
    serde_json::from_value(arguments.unwrap_or_else(|| json!({}))).map_err(invalid_params)
}

fn validate_non_blank(field: &str, value: &str) -> ToolResult<()> {
    if value.trim().is_empty() {
        return Err(invalid_params(format!("{field} must not be blank")));
    }
    Ok(())
}

fn invalid_params(err: impl std::fmt::Display) -> ToolError {
    ToolError::InvalidParams(err.to_string())
}

fn exec_err(err: impl std::fmt::Display) -> ToolError {
    ToolError::Execution(err.to_string())
}

#[cfg(test)]
mod git_ops_tests;
#[cfg(test)]
mod tests;

#[derive(Deserialize)]
struct AgentIdArgs {
    agent_id: String,
}

#[derive(Deserialize)]
struct AnswerArgs {
    agent_id: String,
    text: String,
}

#[derive(Deserialize)]
struct LogArgs {
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct DiffArgs {
    task_id: String,
}
