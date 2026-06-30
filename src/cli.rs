use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(author, version, about = "Repo-oriented bot control and orchestration")]
pub struct Args {
    /// Directory whose direct children should be scanned for git repositories.
    #[arg(default_value = ".")]
    pub launch_dir: PathBuf,

    /// Program to launch for newly-created agents.
    #[arg(long)]
    pub program: Option<String>,

    /// Print discovered repositories and exit.
    #[arg(long)]
    pub list: bool,

    /// Disable best-effort dropr read-only workspace overlay.
    #[arg(long)]
    pub no_dropr: bool,
}
