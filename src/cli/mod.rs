use std::{ffi::OsString, path::PathBuf};

use clap::{Args as ClapArgs, CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};

mod daemon;

pub use daemon::{
    ConfigArgs, ConfigCommand, DecisionsArgs, DecisionsCommand, InboxArgs, InboxCommand,
    OverseerSetting, OverseerStatusArgs, ServiceArgs, ServiceCommand,
};

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

    /// Add an ad-hoc remote ssh destination to the TUI (repeatable).
    #[arg(long)]
    pub host: Vec<String>,

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
    /// Group runtime config toggles: auto-merge, notify channel, protection.
    Config(ConfigArgs),
    /// Run the Overseer daemon in the foreground.
    Daemon,
    /// Print config and state paths.
    Debug,
    /// Manage the decision log.
    Decisions(DecisionsArgs),
    /// Answer whether a worker's shell command is allowed to run. Called by
    /// the agent-client hook robco installs in every worker worktree.
    Guard(GuardArgs),
    /// Manage the Overseer inbox.
    Inbox(InboxArgs),
    /// Register RobCo's MCP server in supported client configs.
    Install(InstallArgs),
    /// Print discovered repositories and exit.
    List(ListArgs),
    /// Run an MCP server over stdio for agent state and control.
    McpStdio,
    /// Create a child agent linked to the calling agent session.
    New(NewArgs),
    /// Terminate all Overseer workers.
    Panic,
    /// Rename a registered repository's local directory.
    Rename(RenameArgs),
    /// Report turn completion to a controller agent.
    Report(ReportArgs),
    /// Remove RobCo's persisted state file.
    Reset,
    /// Restart the daemon: reload the launchd service if one is installed
    /// (bootout, then bootstrap), else explain how to run it.
    Restart,
    /// Manage the launchd service.
    Service(ServiceArgs),
    /// Create an agent in any registered repository.
    Spawn(SpawnArgs),
    /// Start the daemon: load the launchd service if one is installed, else
    /// explain how to run it.
    Start,
    /// Answer whether anything needs you, is stuck, or is running.
    Status(OverseerStatusArgs),
    /// Durably stop the running daemon: bootout the launchd service if one is
    /// installed and loaded, else signal the manually-run process.
    Stop,
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
pub struct RenameArgs {
    /// Registered repository name or absolute path.
    pub repo: String,
    /// New directory name for the repository.
    pub name: String,
}

#[derive(Debug, ClapArgs)]
pub struct GuardArgs {
    /// Which guard to apply to the hook payload on stdin.
    #[arg(value_enum)]
    pub kind: GuardKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GuardKind {
    /// Refuse a command that would end the shared tmux server.
    Tmux,
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
    /// Title for the worker agent. Required unless --dropr-task is given,
    /// which derives it (and conflicts with an explicit title).
    #[arg(long, required_unless_present = "dropr_task")]
    pub title: Option<String>,
    /// Explicit naming slug for branch/worktree/tmux; capped to 32 chars.
    /// Defaults to the sanitized title. Conflicts with --dropr-task, which
    /// derives it.
    #[arg(long)]
    pub name_slug: Option<String>,
    /// Initial prompt for the launched program. Conflicts with --dropr-task,
    /// which derives it.
    #[arg(long)]
    pub prompt: Option<String>,
    /// Parent identity; defaults to ROBCO_AGENT_ID.
    #[arg(long)]
    pub parent: Option<String>,
    /// Launch with the selected profile's autonomous settings.
    #[arg(long)]
    pub autonomous: bool,
    /// Dropr task id (`538` or `#538`) to launch a worker for: claims the
    /// task, and derives --title, --prompt and --name-slug from it. Cannot
    /// be combined with an explicit --title, --prompt, or --name-slug.
    #[arg(long, conflicts_with_all = ["title", "prompt", "name_slug"])]
    pub dropr_task: Option<String>,
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

/// Rewrites a retired `overseer <sub>` invocation into its flat replacement
/// before clap ever sees it, so an already-installed launchd plist (whose
/// `ProgramArguments` still say `overseer run`) keeps starting the daemon
/// without the operator reinstalling the service. Only the two leading
/// tokens are replaced; every flag or positional after the old subcommand
/// name is passed through untouched. Any other `overseer ...` invocation
/// (an unknown sub, or `overseer` alone) is left alone and falls through to
/// clap's normal "unrecognized subcommand" error.
pub(crate) fn rewrite_legacy_overseer(args: &[OsString]) -> Option<Vec<OsString>> {
    if args.get(1).map(OsString::as_os_str) != Some(std::ffi::OsStr::new("overseer")) {
        return None;
    }
    let sub = args.get(2)?.to_str()?;
    let replacement: &[&str] = match sub {
        "run" => &["daemon"],
        "status" => &["status"],
        "stop" => &["stop"],
        "start" => &["start"],
        "restart" => &["restart"],
        "panic" => &["panic"],
        "set" => &["config", "set"],
        "notify-channel" => &["config", "notify-channel"],
        "protection" => &["config", "protection"],
        "clear-inbox" => &["inbox", "clear"],
        "install-service" => &["service", "install"],
        "compact-decisions" => &["decisions", "compact"],
        _ => return None,
    };
    let mut rewritten: Vec<OsString> = args[..1].to_vec();
    rewritten.extend(replacement.iter().map(OsString::from));
    rewritten.extend(args[3..].iter().cloned());
    Some(rewritten)
}

#[cfg(test)]
mod legacy_tests;
#[cfg(test)]
mod spawn_tests;
#[cfg(test)]
mod tests;
