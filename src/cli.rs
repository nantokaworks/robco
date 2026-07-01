use std::path::PathBuf;

use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(author, version, about = "Repo-oriented bot control and orchestration")]
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

    /// Print discovered repositories and exit.
    #[arg(long)]
    pub list: bool,

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
    /// Run an MCP server over stdio for agent state and control.
    McpStdio,
    /// Remove RobCo's persisted state file.
    Reset,
    /// Remove RobCo's MCP server from supported client configs.
    Uninstall(InstallArgs),
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
