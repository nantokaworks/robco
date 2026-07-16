use std::{ffi::OsString, path::PathBuf};

use clap::{Args as ClapArgs, CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    propagate_version = true,
    about = "Repo-oriented bot control and orchestration"
)]
pub struct Args {
    /// Directory whose direct children should be scanned for git repositories.
    #[arg(default_value = ".")]
    pub launch_dir: PathBuf,

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
    #[arg(long, value_enum, default_value_t = InstallTarget::All)]
    pub target: InstallTarget,

    /// Update all supported client configs.
    #[arg(long)]
    pub all: bool,
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
mod tests {
    use super::*;

    #[test]
    fn parses_report_subcommand() {
        let args = Args::try_parse_from([
            "robco",
            "report",
            "--message",
            "turn finished",
            "--target",
            "controller",
        ])
        .unwrap();

        let Some(Command::Report(report)) = args.command else {
            panic!("expected report command");
        };
        assert_eq!(report.message.as_deref(), Some("turn finished"));
        assert_eq!(report.target.as_deref(), Some("controller"));
    }

    #[test]
    fn parses_spawn_subcommand() {
        let args = Args::try_parse_from([
            "robco",
            "spawn",
            "--repo",
            "repo",
            "--title",
            "task",
            "--autonomous",
        ])
        .unwrap();
        let Some(Command::Spawn(args)) = args.command else {
            panic!("expected spawn command")
        };
        assert_eq!(args.repo, "repo");
        assert_eq!(args.title, "task");
        assert!(args.autonomous);
    }

    #[test]
    fn parses_new_subcommand() {
        let args = Args::try_parse_from(["robco", "new", "--title", "x", "--prompt", "y"]).unwrap();
        let Some(Command::New(args)) = args.command else {
            panic!("expected new command");
        };
        assert_eq!(args.title, "x");
        assert_eq!(args.prompt.as_deref(), Some("y"));
    }

    #[test]
    fn parses_version_subcommand() {
        let args = Args::try_parse_from(["robco", "version"]).unwrap();
        assert!(matches!(args.command, Some(Command::Version)));
    }

    #[test]
    fn parses_list_subcommand_with_default_directory() {
        let args = Args::try_parse_from(["robco", "list"]).unwrap();
        let Some(Command::List(args)) = args.command else {
            panic!("expected list command");
        };
        assert_eq!(args.dir, None);
    }

    #[test]
    fn parses_list_subcommand_with_directory() {
        let args = Args::try_parse_from(["robco", "list", "/some/dir"]).unwrap();
        let Some(Command::List(args)) = args.command else {
            panic!("expected list command");
        };
        assert_eq!(args.dir, Some(PathBuf::from("/some/dir")));
    }

    #[test]
    fn parses_list_subcommand_after_launch_directory() {
        let args = Args::try_parse_from(["robco", "/some/dir", "list"]).unwrap();
        assert_eq!(args.launch_dir, PathBuf::from("/some/dir"));
        let Some(Command::List(args)) = args.command else {
            panic!("expected list command");
        };
        assert_eq!(args.dir, None);
    }

    #[test]
    fn rejects_removed_list_flag() {
        assert!(Args::try_parse_from(["robco", "--list"]).is_err());
    }
}
