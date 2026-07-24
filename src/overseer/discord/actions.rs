use super::{commands::Command, handler::CommandExecutor, ledger_requests::LedgerRequest};
use crate::{
    cli::OverseerSetting,
    config::Config,
    overseer::{
        command,
        dispatch::format_dispatch_limit,
        exec::{COMMAND_TIMEOUT, run_timeout},
        is_overseer_child,
        ledger::{Ledger, LedgerPhase},
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
        if let Err(error) = audit(command, user_id, &outcome) {
            return Err(format!("{outcome}; audit failed: {error}"));
        }
        result
    }

    fn refused(&mut self, command: &Command, user_id: &str, reason: &str) {
        if let Err(error) = audit(command, user_id, &format!("refused: {reason}")) {
            eprintln!("overseer: failed to audit Discord refusal: {error}");
        }
    }
}

fn execute(
    command: &Command,
    user_id: &str,
    ledger_requests: &Sender<LedgerRequest>,
) -> crate::Result<String> {
    match command {
        Command::Status => status(),
        Command::Dispatch(enabled) => {
            command::set_runtime(OverseerSetting::Dispatch, *enabled)?;
            Ok(format!("dispatch: {}", on_off(*enabled)))
        }
        Command::AutoMerge(false) => {
            command::set_runtime(OverseerSetting::AutoMerge, false)?;
            Ok("automerge: off".into())
        }
        Command::AutoMerge(true) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "automerge can only be enabled via the local CLI",
        )
        .into()),
        Command::Workers => workers(),
        Command::Tasks => tasks(),
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
        Command::Log(limit) => Ok(format_decisions(*limit)?),
        Command::Panic => {
            command::panic_stop_attributed("discord", Some(user_id))?;
            Ok("panic stop complete".into())
        }
    }
}

fn status() -> crate::Result<String> {
    let config = Config::load()?;
    let ledger = Ledger::load()?;
    let active = ledger
        .entries
        .iter()
        .filter(|entry| !terminal(entry.phase))
        .count();
    Ok(format!(
        "overseer={} dispatch={} automerge={} workers={}/{} today={}/{}",
        on_off(config.overseer.enabled),
        on_off(config.overseer.dispatch_enabled),
        on_off(config.overseer.auto_merge),
        active,
        config.overseer.max_workers,
        ledger.counters.dispatched_today,
        format_dispatch_limit(config.overseer.daily_dispatch_limit)
    ))
}

fn workers() -> crate::Result<String> {
    let registry = Registry::load()?;
    let rows: Vec<_> = registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .filter(|agent| is_overseer_child(agent.parent_agent_id.as_deref()))
        .map(|agent| format!("{}: {:?}", agent.id, agent.status))
        .collect();
    Ok(if rows.is_empty() {
        "no overseer workers".into()
    } else {
        rows.join("\n")
    })
}

fn tasks() -> crate::Result<String> {
    let rows: Vec<_> = Ledger::load()?
        .entries
        .into_iter()
        .map(|entry| format!("{} {} {:?}", entry.display_id, entry.task_id, entry.phase))
        .collect();
    Ok(if rows.is_empty() {
        "no tasks".into()
    } else {
        rows.join("\n")
    })
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

fn format_decisions(limit: usize) -> crate::Result<String> {
    let rows: Vec<_> = logging::tail(limit)?
        .into_iter()
        .map(|entry| {
            format!(
                "{} {:?} {}",
                entry.at.to_rfc3339(),
                entry.kind,
                entry.reason
            )
        })
        .collect();
    Ok(if rows.is_empty() {
        "no decisions".into()
    } else {
        rows.join("\n")
    })
}

fn audit(command: &Command, user_id: &str, outcome: &str) -> crate::Result<()> {
    logging::append(&audit_entry(command, user_id, outcome))
}

fn audit_entry(command: &Command, user_id: &str, outcome: &str) -> DecisionEntry {
    let kind = match command {
        Command::Skip(_) => DecisionKind::Skip,
        Command::Retry(_) => DecisionKind::Dispatch,
        _ => DecisionKind::Hold,
    };
    let mut entry = DecisionEntry::new(kind, format!("command {outcome}: {command:?}"));
    entry.source = Some("discord".into());
    entry.user_id = Some(user_id.into());
    entry.task = match command {
        Command::Skip(task) | Command::Retry(task) => Some(task.clone()),
        _ => None,
    };
    entry
}

fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_entries_identify_discord_user() {
        let entry = audit_entry(&Command::Skip("task-1".into()), "user-7", "failed: denied");
        assert_eq!(entry.source.as_deref(), Some("discord"));
        assert_eq!(entry.user_id.as_deref(), Some("user-7"));
        assert_eq!(entry.task.as_deref(), Some("task-1"));
        assert!(entry.reason.contains("failed: denied"));
    }
}
