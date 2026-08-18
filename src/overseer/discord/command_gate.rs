//! Confirmation and rate-limit gating for `Command`, plus the human-readable
//! description shown in a CONFIRM prompt. Split out of `handler.rs` to keep
//! it under the file-size limit — these three functions are exhaustive
//! matches with no wildcard arm, so a `Command` variant added without an
//! entry here fails to compile.

use super::commands::Command;

pub(super) fn describe_command(command: &Command) -> String {
    match command {
        Command::Status => "status".into(),
        Command::Dispatch(value) => format!("dispatch {}", on_off(*value)),
        Command::AutoMerge(value) => format!("automerge {}", on_off(*value)),
        Command::Workers => "workers".into(),
        Command::Tasks => "tasks".into(),
        Command::Skip(task) => format!("skip {task}"),
        Command::Retry(task) => format!("retry {task}"),
        Command::TaskCreate { repo, title, .. } => format!("create task \"{title}\" in {repo}"),
        Command::Answer { agent, .. } => format!("answer {agent}"),
        Command::Approve(agent) => format!("approve {agent}"),
        Command::Kill(agent) => format!("kill {agent}"),
        Command::Log(limit) => format!("log {limit}"),
        Command::Panic => "panic".into(),
        Command::Merge(task) => format!("merge {task}"),
        Command::Diff(task) => format!("diff {task}"),
        Command::Help => "help".into(),
        Command::Whoami => "whoami".into(),
        Command::Report {
            target_agent_id, ..
        } => match target_agent_id {
            Some(target) => format!("report to {target}"),
            None => "report".into(),
        },
        Command::AgentCreate { repo, title, .. } => format!("create agent \"{title}\" in {repo}"),
        Command::QuestionList => "questions".into(),
        Command::PrStatus(agent) => format!("pr status {agent}"),
        Command::PrRequest { agent, .. } => format!("pr request {agent}"),
        Command::Run(task) => format!("run {task}"),
        Command::Inbox => "inbox".into(),
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

pub(super) fn impactful(command: &Command) -> bool {
    matches!(
        command,
        Command::Kill(_)
            | Command::Panic
            | Command::Retry(_)
            | Command::Skip(_)
            | Command::Approve(_)
            | Command::Answer { .. }
            | Command::Dispatch(true)
            | Command::TaskCreate { .. }
            | Command::Merge(_)
            | Command::Run(_)
    )
    // Risk-reducing `dispatch off` and `automerge off` remain immediate so an
    // incident responder is never delayed by the confirmation round trip.
    //
    // The six commands shared with MCP (Whoami, Report, AgentCreate,
    // QuestionList, PrStatus, PrRequest) are not impactful here either: MCP
    // already runs each of them with no confirmation step, so requiring one
    // only on Discord would make the two surfaces disagree about the same
    // command's risk instead of resolving it.
    //
    // `Run` spawns a worker exactly like `TaskCreate`/`Kill` change something
    // real, so it keeps the confirmation round trip; `Inbox` is a read.
}

pub(super) fn mutating(command: &Command) -> bool {
    !matches!(
        command,
        Command::Status
            | Command::Workers
            | Command::Tasks
            | Command::Log(_)
            | Command::Diff(_)
            | Command::Help
            | Command::Whoami
            | Command::QuestionList
            | Command::PrStatus(_)
            | Command::Inbox
    )
}

#[cfg(test)]
mod tests {
    use super::super::help::samples;
    use super::*;

    /// Every `Command` variant must resolve through all three gates without
    /// panicking — the exhaustive matches above already guarantee that at
    /// compile time, but this exercises every declared sample too.
    #[test]
    fn every_command_variant_resolves_through_every_gate() {
        for command in samples() {
            let _ = describe_command(&command);
            let _ = impactful(&command);
            let _ = mutating(&command);
        }
    }

    #[test]
    fn the_new_shared_commands_are_not_impactful() {
        for command in [
            Command::Whoami,
            Command::Report {
                message: String::new(),
                target_agent_id: None,
            },
            Command::AgentCreate {
                repo: String::new(),
                title: String::new(),
                prompt: None,
                parent_agent_id: None,
                autonomous: false,
            },
            Command::QuestionList,
            Command::PrStatus(String::new()),
            Command::PrRequest {
                agent: String::new(),
                prompt: None,
            },
        ] {
            assert!(!impactful(&command));
        }
    }
}
