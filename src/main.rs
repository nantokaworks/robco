mod agent;
mod cli;
mod config;
mod discover;
mod dropr;
mod git;
mod model;
mod registry;
mod status;
mod tmux;
mod ui;

use std::{path::PathBuf, process::ExitCode};

use clap::Parser;
use cli::Args;
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
    if args.no_dropr {
        config.dropr_overlay = false;
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
    ui::run(registry, config)
}
