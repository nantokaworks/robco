use std::{ffi::OsString, path::PathBuf};

use clap::{Args as ClapArgs, CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};

use crate::overseer::{autonomy::AutonomyLevel, config::ProtectionMode};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    propagate_version = true,
    about = "Repo-oriented bot control and orchestration"
)]
pub struct Args {
    /// Directory whose direct children should be scanned for git repositories.
    pub launch_dir: Option<PathBuf>,

    /// Program to launch for newly-created agents.
    #[arg(long)]
    pub program: Option<String>,

    /// Automatically answer common permission prompts with "y".
    #[arg(short = 'y', long = "autoyes")]
    pub auto_yes: bool,

    /// Disable best-effort dropr read-only workspace overlay.
    #[arg(long)]
    pub no_dropr: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Clone a git repository into the managed repos directory.
    Add(AddArgs),
    /// Run and administer the autonomous Overseer control plane.
    Overseer(OverseerArgs),
    /// Print config and state paths.
    Debug,
    /// Register RobCo's MCP server in supported client configs.
    Install(InstallArgs),
    /// Print discovered repositories and exit.
    List(ListArgs),
    /// Run an MCP server over stdio for agent state and control.
    McpStdio,
    /// Create a child agent linked to the calling agent session.
    New(NewArgs),
    /// Report turn completion to a controller agent.
    Report(ReportArgs),
    /// Create an agent in any registered repository.
    Spawn(SpawnArgs),
    /// Remove RobCo's persisted state file.
    Reset,
    /// Remove RobCo's MCP server from supported client configs.
    Uninstall(InstallArgs),
    /// Print version information.
    Version,
}

#[derive(Debug, ClapArgs)]
pub struct AddArgs {
    /// Git URL to clone.
    pub url: String,
    /// Branch to check out while cloning.
    #[arg(long)]
    pub branch: Option<String>,
    /// Destination directory name under repos_root.
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerArgs {
    #[command(subcommand)]
    pub command: OverseerCommand,
}

#[derive(Debug, Subcommand)]
pub enum OverseerCommand {
    /// Run the Overseer daemon in the foreground.
    Run,
    /// Answer whether anything needs you, is stuck, or is running.
    Status(OverseerStatusArgs),
    /// Durably stop the running daemon: bootout the launchd service if one is
    /// installed and loaded, else signal the manually-run process.
    Stop,
    /// Start the daemon: load the launchd service if one is installed, else
    /// explain how to run it.
    Start,
    /// Restart the daemon: reload the launchd service if one is installed
    /// (bootout, then bootstrap), else explain how to run it.
    Restart,
    /// Persist a runtime toggle in RobCo's JSON config.
    Set(OverseerSetArgs),
    /// Set the daily dispatch limit (0 = unlimited).
    DailyLimit(OverseerDailyLimitArgs),
    /// Choose the channel that receives reports (decision notifications and
    /// digests). Cleared, reports fall back to the chat channel.
    NotifyChannel(OverseerNotifyChannelArgs),
    /// Set how strictly auto-merge requires the base branch to be protected.
    Protection(OverseerProtectionArgs),
    /// Set how much of the merge envelope the daemon may clear on its own.
    Autonomy(OverseerAutonomyArgs),
    /// Disable dispatch and terminate all Overseer workers.
    Panic,
    /// Hide every currently listed Inbox item. Suppression only: the decision
    /// log and the ledger are left alone, and a newer escalation for the same
    /// target is listed again.
    ClearInbox,
    /// Write a launchd service plist and load or reload it after confirmation.
    InstallService,
    /// Quarantine unparseable decision-log lines to a sidecar file, keeping
    /// every valid line byte-identical and in order. Safe to run while the
    /// daemon is appending.
    CompactDecisions(OverseerCompactDecisionsArgs),
}

#[derive(Debug, ClapArgs)]
pub struct OverseerSetArgs {
    #[arg(value_enum)]
    pub setting: OverseerSetting,
    #[arg(value_enum)]
    pub value: OnOff,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerStatusArgs {
    /// Also print internal bookkeeping: the raw ledger phase tally, `workers by
    /// repo` keyed by absolute path, the skip list, and the recent decision-log
    /// tail. Nothing here is deleted from the daemon's records — this only
    /// changes what the command prints by default.
    #[arg(long)]
    pub debug: bool,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerDailyLimitArgs {
    /// Maximum worker dispatches per day; 0 means unlimited.
    pub value: u32,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerNotifyChannelArgs {
    /// Discord channel id that receives reports.
    #[arg(required_unless_present = "clear")]
    pub channel_id: Option<String>,
    /// Clear the report channel; reports fall back to the chat channel.
    #[arg(long, conflicts_with = "channel_id")]
    pub clear: bool,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerProtectionArgs {
    /// `required` demands a pull-request rule and required status checks, `relaxed`
    /// demands only the pull-request rule, `off` skips the probe entirely.
    #[arg(value_enum)]
    pub mode: ProtectionMode,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerAutonomyArgs {
    /// `approval_only` escalates every merge, `conservative` auto-merges only a
    /// risk-free docs-or-tests change, `full_auto` escalates the hard stops alone.
    #[arg(value_enum)]
    pub level: AutonomyLevel,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerCompactDecisionsArgs {
    /// Report kept/quarantined counts without rewriting the log.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OverseerSetting {
    Dispatch,
    AutoMerge,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OnOff {
    On,
    Off,
}

impl OnOff {
    pub fn enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

#[derive(Debug, ClapArgs)]
pub struct ListArgs {
    /// Directory whose direct children should be scanned for git repositories.
    pub dir: Option<PathBuf>,
}

#[derive(Debug, ClapArgs)]
pub struct NewArgs {
    /// Title for the child agent.
    #[arg(short, long)]
    pub title: String,

    /// Initial prompt for the launched program.
    #[arg(long)]
    pub prompt: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct ReportArgs {
    /// Report text. Exit codes: 0 delivered, 2 busy, 3 invalid, 4 unavailable.
    #[arg(short, long, required_unless_present = "kind")]
    pub message: Option<String>,

    /// Lifecycle report kind used by autonomous agent hooks.
    #[arg(long, conflicts_with = "message", required_unless_present = "message")]
    pub kind: Option<String>,

    /// Agent id to report to; defaults to ROBCO_PARENT_AGENT_ID.
    #[arg(long)]
    pub target: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct SpawnArgs {
    /// Registered repository name or absolute path.
    #[arg(long)]
    pub repo: String,
    /// Title for the worker agent.
    #[arg(long)]
    pub title: String,
    /// Explicit naming slug for branch/worktree/tmux; capped to 32 chars. Defaults to the sanitized title.
    #[arg(long)]
    pub name_slug: Option<String>,
    /// Initial prompt for the launched program.
    #[arg(long)]
    pub prompt: Option<String>,
    /// Parent identity; defaults to ROBCO_AGENT_ID.
    #[arg(long)]
    pub parent: Option<String>,
    /// Launch with the selected profile's autonomous settings.
    #[arg(long)]
    pub autonomous: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum InstallTarget {
    Claude,
    Codex,
    Openclaw,
    All,
}

#[derive(Debug, ClapArgs)]
pub struct InstallArgs {
    /// Client config to update.
    #[arg(long, value_enum)]
    pub target: Option<InstallTarget>,

    /// Update all supported client configs.
    #[arg(long)]
    pub all: bool,
}

impl InstallArgs {
    pub fn wants_wizard(&self) -> bool {
        self.target.is_none() && !self.all
    }
}

pub(crate) fn invocation_targets_report(args: &[OsString]) -> bool {
    Args::command()
        .ignore_errors(true)
        .try_get_matches_from(args)
        .is_ok_and(|matches| matches.subcommand_name() == Some("report"))
}

pub(crate) fn report_parse_error_message(
    error: &clap::Error,
    invocation_targets_report: bool,
) -> Option<&'static str> {
    if invocation_targets_report
        && !matches!(
            error.kind(),
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
        )
    {
        Some("robco report: invalid arguments (see --help)")
    } else {
        None
    }
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
