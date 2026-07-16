mod agent;
mod chief;
mod cli;
mod config;
mod discover;
mod dropr;
mod git;
mod loading;
mod mcp;
mod model;
mod new_agent;
mod notify;
mod openclaw;
mod registry;
mod setup;
mod spawn;
mod status;
pub mod subagents;
mod tmux;
mod ui;
mod version;

use std::{ffi::OsString, path::PathBuf, process::ExitCode};

use clap::{Parser, error::ErrorKind};
use cli::{Args, Command, ReportArgs};
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
    #[error("child worktrees remain under {0}; remove them first")]
    ChildWorktreesPresent(PathBuf),
    #[error("robco new must run inside a robco agent session (ROBCO_AGENT_ID is not set)")]
    NewOutsideAgentSession,
    #[error("parent robco agent not found in registry: {0}")]
    ParentAgentNotFound(String),
    #[error("registered repository not found: {0}")]
    RepoSelectorNotFound(String),
    #[error("repository name is ambiguous; use an absolute path: {0}")]
    RepoSelectorAmbiguous(String),
    #[error(
        "child worktree {worktree_path} and tmux session {tmux_session} were created, but the \
         repository disappeared from the registry; the TUI will adopt the child"
    )]
    CreatedChildRepoMissing {
        worktree_path: PathBuf,
        tmux_session: String,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let args = match Args::try_parse_from(&raw_args) {
        Ok(args) => args,
        Err(err) => {
            if err.kind() == ErrorKind::DisplayVersion {
                version::print();
                return ExitCode::SUCCESS;
            }
            if let Some(message) =
                cli::report_parse_error_message(&err, cli::invocation_targets_report(&raw_args))
            {
                eprintln!("{message}");
                return ExitCode::from(3);
            }
            err.exit();
        }
    };
    if matches!(&args.command, Some(Command::Version)) {
        version::print();
        return ExitCode::SUCCESS;
    }
    if let Some(Command::Report(report_args)) = &args.command {
        return run_report(report_args);
    }

    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("robco: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<()> {
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
        if matches!(
            command,
            Command::Chief(cli::ChiefArgs {
                command: cli::ChiefCommand::Run
            })
        ) {
            return chief::daemon::run_daemon().await;
        }
        return run_command(command, &config, &args.launch_dir);
    }

    let indicator = loading::Indicator::start("Scanning repositories...");
    let discovered = discover_with_overlay(&args.launch_dir, &config, &indicator)?;

    let registry = Registry::locked_update(|registry| {
        agent::normalize_adopted_titles(&mut registry.repos, &config);
        registry.merge_discovered(discovered);
    })?;
    let launch_dir = args.launch_dir.canonicalize().unwrap_or(args.launch_dir);
    indicator.finish();
    ui::run(registry, config, launch_dir)
}

fn run_command(command: Command, config: &Config, launch_dir: &std::path::Path) -> Result<()> {
    match command {
        Command::Chief(args) => chief::command::run(args, config)?,
        Command::Debug => {
            println!("config: {}", config::config_file_path()?.display());
            println!("state: {}", config::state_path()?.display());
            println!("worktrees: {}", config.worktree_root.display());
            println!("program: {}", config.default_program_command());
            println!("dropr_overlay: {}", config.dropr_overlay);
            println!("auto_accept: {}", config.auto_accept);
        }
        Command::Install(args) => setup::install(&args)?,
        Command::List(args) => {
            let dir = resolve_list_dir(args.dir.as_deref(), launch_dir);
            list_repositories(dir, config)?;
        }
        Command::McpStdio => unreachable!("mcp-stdio is handled before sync commands"),
        Command::New(args) => new_agent::run(args, config)?,
        Command::Report(_) => unreachable!("report is handled before config loading"),
        Command::Spawn(args) => {
            let parent = args.parent.or_else(|| {
                std::env::var(config::ENV_AGENT_ID)
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            });
            let extra_args = if args.autonomous {
                config
                    .profiles
                    .iter()
                    .find(|profile| profile.name == config.default_program)
                    .map(|profile| profile.autonomous_args.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let outcome = spawn::spawn_in_repo_with_mode(
                &args.repo,
                &args.title,
                args.prompt.as_deref(),
                parent.as_deref(),
                &extra_args,
                args.autonomous,
                config,
            )?;
            println!("id: {}", outcome.id);
            println!("branch: {}", outcome.branch);
            println!("worktree: {}", outcome.worktree_path.display());
            println!("tmux: {}", outcome.tmux_session);
        }
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
        Command::Version => unreachable!("version is handled before config loading"),
    }
    Ok(())
}

fn resolve_list_dir<'a>(
    list_dir: Option<&'a std::path::Path>,
    launch_dir: &'a std::path::Path,
) -> &'a std::path::Path {
    list_dir.unwrap_or(launch_dir)
}

fn discover_with_overlay(
    dir: &std::path::Path,
    config: &Config,
    indicator: &loading::Indicator,
) -> Result<Vec<model::RepoNode>> {
    let mut discovered = discover::discover_repos(dir)?;
    if config.dropr_overlay {
        indicator.set_message("Loading dropr workspaces...");
        let overlay = dropr::DroprOverlay::load_best_effort();
        for repo in &mut discovered {
            if let Some(remote) = &repo.remote_url {
                repo.dropr = overlay.find_by_repo_url(remote).cloned();
            }
        }
    }
    Ok(discovered)
}

fn list_repositories(dir: &std::path::Path, config: &Config) -> Result<()> {
    let indicator = loading::Indicator::start("Scanning repositories...");
    let discovered = discover_with_overlay(dir, config, &indicator)?;
    indicator.finish();
    for repo in &discovered {
        let remote = repo.remote_url.as_deref().unwrap_or("-");
        let dropr = repo
            .dropr
            .as_ref()
            .map(|workspace| format!(" dropr:{}", workspace.id))
            .unwrap_or_default();
        println!("{}\t{}{}", repo.path.display(), remote, dropr);
    }
    Ok(())
}

fn run_report(args: &ReportArgs) -> ExitCode {
    let message = args
        .message
        .as_deref()
        .or(args.kind.as_deref())
        .unwrap_or("");
    if message.is_empty() {
        eprintln!("robco report: --message or --kind is required");
        return ExitCode::from(3);
    }
    match mcp::deliver_report(message, args.target.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let message: String = err
                .to_string()
                .chars()
                .map(|character| {
                    if character.is_control() {
                        ' '
                    } else {
                        character
                    }
                })
                .collect();
            eprintln!("robco: {message}");
            ExitCode::from(mcp::report_exit_code(&err))
        }
    }
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
