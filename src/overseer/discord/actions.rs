use super::{
    agent_actions, command_gate::describe_command, commands::Command, handler::CommandExecutor,
    help, inbox_reply, ledger_requests::LedgerRequest, merge_actions, reports,
    task_create::create_task,
};
use crate::{
    cli::OverseerSetting,
    overseer::{
        command,
        exec::{COMMAND_TIMEOUT, run_timeout},
        logging::{self, DecisionEntry, DecisionKind},
    },
    registry::Registry,
};
use std::{process::Command as ProcessCommand, sync::mpsc::Sender};

pub struct SystemExecutor {
    ledger_requests: Sender<LedgerRequest>,
}

impl SystemExecutor {
    pub(crate) fn new(ledger_requests: Sender<LedgerRequest>) -> Self {
        Self { ledger_requests }
    }
}

impl CommandExecutor for SystemExecutor {
    fn execute(&mut self, command: &Command, user_id: &str) -> Result<String, String> {
        let result =
            execute(command, user_id, &self.ledger_requests).map_err(|error| error.to_string());
        let outcome = match &result {
            Ok(_) => "succeeded".to_string(),
            Err(error) => format!("failed/refused: {error}"),
        };
        if let Err(error) = audit(command, user_id, "discord", &outcome) {
            return Err(format!("{outcome}; audit failed: {error}"));
        }
        result
    }

    fn refused(&mut self, command: &Command, user_id: &str, reason: &str) {
        if let Err(error) = audit(command, user_id, "discord", &format!("refused: {reason}")) {
            eprintln!("overseer: failed to audit Discord refusal: {error}");
        }
    }
}

/// The one place every `Command` variant is executed, whatever surface asked
/// for it. Discord's `SystemExecutor` wraps this with its CONFIRM-nonce gate
/// and audit logging; `crate::mcp::tools` calls it directly for the variants
/// that carry no such gate (see `command_gate::impactful` for which ones do)
/// — see `dropr:463` for the design record on which variants stay
/// surface-specific instead of routing through here.
pub(crate) fn execute(
    command: &Command,
    user_id: &str,
    ledger_requests: &Sender<LedgerRequest>,
) -> crate::Result<String> {
    match command {
        Command::Status => reports::status(),
        Command::AutoMerge(false) => {
            command::set_runtime(OverseerSetting::AutoMerge, false)?;
            Ok("automerge: off".into())
        }
        Command::AutoMerge(true) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "automerge can only be enabled via the local CLI",
        )
        .into()),
        Command::Workers => reports::workers(),
        Command::Tasks => reports::tasks(),
        Command::Skip(task) => queue_ledger(
            ledger_requests,
            LedgerRequest::Skip {
                task: task.clone(),
                user_id: user_id.into(),
            },
            task,
        ),
        Command::Retry(task) => queue_ledger(
            ledger_requests,
            LedgerRequest::Retry {
                task: task.clone(),
                user_id: user_id.into(),
            },
            task,
        ),
        Command::TaskCreate {
            repo,
            title,
            description,
        } => create_task(repo, title, description.as_deref()),
        Command::Answer { agent, text } => {
            let session = agent_session(agent)?;
            tmux(&["send-keys", "-t", &format!("={session}:"), "-l", "--", text])?;
            tmux(&["send-keys", "-t", &format!("={session}"), "Enter"])?;
            Ok(format!("answered {agent}"))
        }
        Command::Approve(agent) => {
            tmux(&[
                "send-keys",
                "-t",
                &format!("={}", agent_session(agent)?),
                "y",
                "Enter",
            ])?;
            Ok(format!("approved {agent}"))
        }
        Command::Kill(agent) => {
            tmux(&["kill-session", "-t", &format!("={}", agent_session(agent)?)])?;
            Ok(format!("killed {agent}"))
        }
        Command::Log(limit) => reports::format_decisions(*limit),
        Command::Panic => {
            command::panic_stop_attributed("discord", Some(user_id))?;
            Ok("panic stop complete".into())
        }
        Command::Merge(task) => merge_actions::merge(task, user_id, ledger_requests),
        Command::Diff(task) => merge_actions::diff(task),
        Command::Help => Ok(help::help_message()),
        Command::Whoami => agent_actions::whoami(),
        Command::Report {
            message,
            target_agent_id,
        } => agent_actions::report(message, target_agent_id.as_deref()),
        Command::AgentCreate {
            repo,
            title,
            prompt,
            parent_agent_id,
            autonomous,
        } => agent_actions::agent_create(
            repo,
            title,
            prompt.as_deref(),
            parent_agent_id.as_deref(),
            *autonomous,
        ),
        Command::QuestionList => agent_actions::question_list(),
        Command::PrStatus(agent) => agent_actions::pr_status(agent),
        Command::PrRequest { agent, prompt } => agent_actions::pr_request(agent, prompt.as_deref()),
        Command::Run(task) => queue_ledger(
            ledger_requests,
            LedgerRequest::Run {
                task: task.clone(),
                user_id: user_id.into(),
            },
            task,
        ),
        Command::Inbox => inbox_reply::inbox(),
    }
}

fn queue_ledger(
    sender: &Sender<LedgerRequest>,
    request: LedgerRequest,
    task: &str,
) -> crate::Result<String> {
    sender.send(request).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "daemon ledger request channel is closed",
        )
    })?;
    Ok(format!("{task}: queued"))
}

fn tmux(args: &[&str]) -> crate::Result<()> {
    let mut command = ProcessCommand::new("tmux");
    command.args(args);
    let output = run_timeout(command, COMMAND_TIMEOUT)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "tmux exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
        .into())
    }
}

fn agent_session(agent_id: &str) -> crate::Result<String> {
    Registry::load()?
        .repos
        .into_iter()
        .flat_map(|repo| repo.agents)
        .find(|agent| agent.id == agent_id)
        .map(|agent| agent.tmux_session)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "agent not found").into())
}

/// `source` is the caller's identity for the decision log (`"discord"` for
/// `SystemExecutor`; `crate::mcp::tools` passes `"mcp"` when it calls
/// [`execute`] directly), so the two surfaces stay distinguishable in
/// `decisions.jsonl` even though they share this one audit path.
pub(crate) fn audit(
    command: &Command,
    user_id: &str,
    source: &str,
    outcome: &str,
) -> crate::Result<()> {
    logging::append(&audit_entry(command, user_id, source, outcome))
}

fn audit_entry(command: &Command, user_id: &str, source: &str, outcome: &str) -> DecisionEntry {
    let kind = match command {
        Command::Skip(_) => DecisionKind::Skip,
        Command::Retry(_) => DecisionKind::Dispatch,
        Command::Merge(_) => DecisionKind::Merge,
        _ => DecisionKind::Hold,
    };
    let mut entry = DecisionEntry::new(
        kind,
        format!("command {outcome}: {}", describe_command(command)),
    );
    entry.source = Some(source.into());
    entry.user_id = Some(user_id.into());
    entry.task = match command {
        Command::Skip(task)
        | Command::Retry(task)
        | Command::Merge(task)
        | Command::Diff(task)
        | Command::Run(task) => Some(task.clone()),
        _ => None,
    };
    entry
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
