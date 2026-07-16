use std::time::SystemTime;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    model::{AgentNode, RepoNode, Status},
    registry::Registry,
    status::{self, StatusReport, WatchStatusState},
    subagents::{SubagentReader, SubagentStatus, claude::ClaudeSubagentReader, read_allowed},
    tmux,
};

const PROMPT_LINES: usize = 20;

mod catalog;
mod identity;
mod policy;
mod report;
mod spawn;

pub use catalog::list_tools;
pub(crate) use report::{deliver_report, report_exit_code};

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
        "robco_chief_policy" => {
            let args: policy::PolicyArgs = parse_args(arguments)?;
            policy::policy(args)
        }
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
        "robco_question_list" => {
            let registry = Registry::load().map_err(exec_err)?;
            question_list(&registry)
        }
        "robco_answer" => {
            let args: AnswerArgs = parse_args(arguments)?;
            validate_non_blank("agent_id", &args.agent_id)?;
            validate_non_blank("text", &args.text)?;
            let registry = Registry::load().map_err(exec_err)?;
            answer(&registry, &args.agent_id, &args.text)
        }
        "robco_approve" => {
            let args: AgentIdArgs = parse_args(arguments)?;
            validate_non_blank("agent_id", &args.agent_id)?;
            let registry = Registry::load().map_err(exec_err)?;
            approve(&registry, &args.agent_id)
        }
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
        ClaudeSubagentReader::default().read(&agent.worktree_path, SystemTime::now())
    } else {
        agent.subagents.clone()
    };
    let subagents_active = subagents
        .iter()
        .filter(|subagent| subagent.status == SubagentStatus::Running)
        .count();
    let tracked_command = agent.tracked_command.clone().or_else(|| {
        let pane_pid = tmux::pane_pid(&agent.tmux_session).ok().flatten()?;
        status::proc::ProcSnapshot::capture()
            .ok()?
            .tracked_command(pane_pid)
    });
    (tracked_command, subagents_active)
}

fn question_list(registry: &Registry) -> ToolResult<Value> {
    let questions = registry
        .repos
        .iter()
        .flat_map(|repo| {
            repo.agents.iter().filter_map(move |agent| {
                let report = live_status(repo, agent);
                (report.status == Status::Waiting && report.awaiting_confirmation).then(|| {
                    json!({
                        "agent_id": agent.id,
                        "title": agent.title,
                        "tmux_session": agent.tmux_session,
                        "worktree_missing": report.worktree_missing,
                        "prompt": prompt_tail(&agent.tmux_session)
                    })
                })
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "questions": questions }))
}

fn answer(registry: &Registry, agent_id: &str, text: &str) -> ToolResult<Value> {
    let (_, agent) = find_agent(registry, agent_id)?;
    tmux::send_literal_text(&agent.tmux_session, text).map_err(exec_err)?;
    tmux::send_keys(&agent.tmux_session, &["Enter"]).map_err(exec_err)?;
    Ok(json!({ "ok": true }))
}

fn approve(registry: &Registry, agent_id: &str) -> ToolResult<Value> {
    let (_, agent) = find_agent(registry, agent_id)?;
    tmux::send_keys(&agent.tmux_session, &["y", "Enter"]).map_err(exec_err)?;
    Ok(json!({ "ok": true }))
}

fn live_status(repo: &RepoNode, agent: &AgentNode) -> StatusReport {
    let mut state = WatchStatusState::default();
    status::classify_agent_status(
        &repo.path,
        &agent.worktree_path,
        &agent.branch,
        &agent.tmux_session,
        &mut state,
    )
    .unwrap_or(StatusReport {
        status: Status::Dead,
        awaiting_confirmation: false,
        worktree_missing: false,
    })
}

fn prompt_tail(session: &str) -> String {
    tmux::capture_plain(session)
        .or_else(|_| tmux::capture_text(session))
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
