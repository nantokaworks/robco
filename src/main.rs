mod agent;
mod cli;
mod config;
mod discover;
mod dropr;
mod git;
mod mcp;
mod model;
mod notify;
mod registry;
mod setup;
mod status;
mod tmux;
mod ui;

use std::{path::PathBuf, process::ExitCode};

use clap::Parser;
use cli::{Args, Command};
use config::Config;
use registry::Registry;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml error: {0}")]
    Toml(#[from] toml_edit::TomlError),
    #[error("home directory could not be resolved")]
    HomeDir,
    #[error("{context} failed: {stderr}")]
    Command {
        context: &'static str,
        stderr: String,
    },
    #[error("worktree has tracked changes: {0}")]
    DirtyWorktree(PathBuf),
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("robco: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let args = Args::parse();
    let mut config = Config::load()?;
    if let Some(program) = args.program {
        config.default_program = program;
    }
    if args.auto_yes {
        config.auto_accept = true;
    }
    if args.no_dropr {
        config.dropr_overlay = false;
    }

    if let Some(command) = args.command {
        if matches!(command, Command::McpStdio) {
            return mcp::run_stdio();
        }
        return run_command(command, &config);
    }

    let mut discovered = discover::discover_repos(&args.launch_dir)?;
    if config.dropr_overlay {
        let overlay = dropr::DroprOverlay::load_best_effort();
        for repo in &mut discovered {
            if let Some(remote) = &repo.remote_url {
                repo.dropr = overlay.find_by_repo_url(remote).cloned();
            }
        }
    }

    if args.list {
        for repo in &discovered {
            let remote = repo.remote_url.as_deref().unwrap_or("-");
            let dropr = repo
                .dropr
                .as_ref()
                .map(|workspace| format!(" dropr:{}", workspace.id))
                .unwrap_or_default();
            println!("{}\t{}{}", repo.path.display(), remote, dropr);
        }
        return Ok(());
    }

    let mut registry = Registry::load()?;
    registry.merge_discovered(discovered);
    registry.save()?;
    let launch_dir = args.launch_dir.canonicalize().unwrap_or(args.launch_dir);
    ui::run(registry, config, launch_dir)
}

fn run_command(command: Command, config: &Config) -> Result<()> {
    match command {
        Command::Debug => {
            println!("config: {}", config::config_file_path()?.display());
            println!("state: {}", config::state_path()?.display());
            println!("worktrees: {}", config.worktree_root.display());
            println!("program: {}", config.default_program_command());
            println!("dropr_overlay: {}", config.dropr_overlay);
            println!("auto_accept: {}", config.auto_accept);
        }
        Command::Install(args) => setup::install(&args)?,
        Command::McpStdio => unreachable!("mcp-stdio is handled before sync commands"),
        Command::Reset => {
            let path = config::state_path()?;
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("removed {}", path.display());
            } else {
                println!("state file not found: {}", path.display());
            }
        }
        Command::Uninstall(args) => setup::uninstall(&args)?,
    }
    Ok(())
}
