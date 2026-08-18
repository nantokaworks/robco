//! `!help`: lists every Discord command, built from the `Command` enum so a
//! new command cannot land without a help entry — see `describe`'s
//! exhaustive match below.

use super::commands::Command;

pub(super) struct Entry {
    pub usage: &'static str,
    pub description: &'static str,
}

/// One entry per `Command` variant. The match has no wildcard arm, so a
/// variant added to `Command` without a matching arm here fails to compile
/// — see `every_command_variant_has_a_help_entry` in the test module, which
/// exercises one sample of every variant through this function.
pub(super) fn describe(command: &Command) -> Entry {
    match command {
        Command::Status => Entry {
            usage: "!status",
            description: "Show dispatch, automerge, worker count, and today's dispatch count.",
        },
        Command::Help => Entry {
            usage: "!help",
            description: "List every Discord command.",
        },
        Command::Dispatch(_) => Entry {
            usage: "!dispatch <on|off>",
            description: "Turn automatic dispatch on or off.",
        },
        Command::AutoMerge(_) => Entry {
            usage: "!automerge off",
            description: "Turn auto-merge off. Turning it on from Discord is refused.",
        },
        Command::Workers => Entry {
            usage: "!workers",
            description: "List Overseer worker agents and their status.",
        },
        Command::Tasks => Entry {
            usage: "!tasks",
            description: "List ledger entries and their phase.",
        },
        Command::Skip(_) => Entry {
            usage: "!skip <task>",
            description: "Skip a task so Overseer stops dispatching it.",
        },
        Command::Retry(_) => Entry {
            usage: "!retry <task>",
            description: "Reset a task's retry count so Overseer tries it again.",
        },
        Command::TaskCreate { .. } => Entry {
            usage: "create task <repo> <title> [description]",
            description: "Create a dropr task. Conversational only, no ! syntax.",
        },
        Command::Answer { .. } => Entry {
            usage: "!answer <agent> <text>",
            description: "Send text into a worker's tmux session.",
        },
        Command::Approve(_) => Entry {
            usage: "!approve <agent>",
            description: "Answer a worker's yes/no prompt with y.",
        },
        Command::Kill(_) => Entry {
            usage: "!kill <agent>",
            description: "Kill a worker's tmux session.",
        },
        Command::Log(_) => Entry {
            usage: "!log [limit]",
            description: "Show the last decisions from the decision log (default 10, max 50).",
        },
        Command::Panic => Entry {
            usage: "!panic",
            description: "Stop dispatch and request every worker to stop.",
        },
        Command::Merge(_) => Entry {
            usage: "!merge <task>",
            description: "Queue an approval for a task's pull request; the daemon's merge pass \
                merges it. Requires confirmation.",
        },
        Command::Diff(_) => Entry {
            usage: "!diff <task>",
            description: "Show what an escalated task's pull request changes.",
        },
    }
}

/// One representative value per `Command` variant, in enum declaration
/// order. Field values are placeholders — `describe` never reads them.
pub(super) fn samples() -> Vec<Command> {
    vec![
        Command::Status,
        Command::Dispatch(false),
        Command::AutoMerge(false),
        Command::Workers,
        Command::Tasks,
        Command::Skip(String::new()),
        Command::Retry(String::new()),
        Command::TaskCreate {
            repo: String::new(),
            title: String::new(),
            description: None,
        },
        Command::Answer {
            agent: String::new(),
            text: String::new(),
        },
        Command::Approve(String::new()),
        Command::Kill(String::new()),
        Command::Log(10),
        Command::Panic,
        Command::Merge(String::new()),
        Command::Diff(String::new()),
        Command::Help,
    ]
}

/// The full `!help` reply, one line per command. Discord's message-length
/// cap is handled generically by `gateway::send_text`'s call to
/// `message_split::split_message` — the same path every other reply goes
/// through — so nothing here needs to chunk the text itself.
pub(super) fn help_message() -> String {
    samples()
        .iter()
        .map(describe)
        .map(|entry| format!("`{}` — {}", entry.usage, entry.description))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "help_tests.rs"]
mod tests;
