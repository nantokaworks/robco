mod agent;
mod cli;
mod config;
mod discover;
mod dropr;
mod git;
mod loading;
mod mcp;
mod model;
mod notify;
mod openclaw;
mod registry;
mod setup;
mod status;
mod tmux;
mod ui;

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
}

#[tokio::main]
async fn main() -> ExitCode {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let args = match Args::try_parse_from(&raw_args) {
        Ok(args) => args,
        Err(err) => {
            if let Some(message) =
                report_parse_error_message(&err, invocation_targets_report(&raw_args))
            {
                eprintln!("{message}");
                return ExitCode::from(3);
            }
            err.exit();
        }
    };
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

fn invocation_targets_report(args: &[OsString]) -> bool {
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "report" {
            return true;
        }
        if arg == "--program" {
            args.next();
            continue;
        }
        if arg.to_string_lossy().starts_with("--program=")
            || matches!(
                arg.to_str(),
                Some("-y" | "--autoyes" | "--list" | "--no-dropr")
            )
        {
            continue;
        }
        return false;
    }
    false
}

fn report_parse_error_message(
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
        return run_command(command, &config);
    }

    let indicator = loading::Indicator::start("Scanning repositories...");
    let mut discovered = discover::discover_repos(&args.launch_dir)?;
    if config.dropr_overlay {
        indicator.set_message("Loading dropr workspaces...");
        let overlay = dropr::DroprOverlay::load_best_effort();
        for repo in &mut discovered {
            if let Some(remote) = &repo.remote_url {
                repo.dropr = overlay.find_by_repo_url(remote).cloned();
            }
        }
    }

    if args.list {
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
        return Ok(());
    }

    let mut registry = Registry::load()?;
    agent::normalize_adopted_titles(&mut registry.repos, &config);
    registry.merge_discovered(discovered);
    registry.save()?;
    let launch_dir = args.launch_dir.canonicalize().unwrap_or(args.launch_dir);
    indicator.finish();
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
        Command::Report(_) => unreachable!("report is handled before config loading"),
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

fn run_report(args: &ReportArgs) -> ExitCode {
    match mcp::deliver_report(&args.message, args.target.as_deref()) {
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
mod tests {
    use super::*;

    fn parse_error(args: &[&str]) -> clap::Error {
        Args::try_parse_from(args).unwrap_err()
    }

    fn mapped_report_error(args: &[&str]) -> Option<&'static str> {
        let raw_args = args.iter().map(OsString::from).collect::<Vec<_>>();
        report_parse_error_message(&parse_error(args), invocation_targets_report(&raw_args))
    }

    #[test]
    fn report_missing_message_maps_to_one_line_exit_three_error() {
        let message = mapped_report_error(&["robco", "report"]).unwrap();
        assert_eq!(message.lines().count(), 1);
        assert_eq!(message, "robco report: invalid arguments (see --help)");
    }

    #[test]
    fn report_unknown_flag_maps_to_exit_three_error() {
        assert!(mapped_report_error(&["robco", "report", "--unknown"]).is_some());
    }

    #[test]
    fn report_help_keeps_clap_output_and_success_exit() {
        let args = ["robco", "report", "--help"];
        let error = parse_error(&args);
        let raw_args = args.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        assert_eq!(error.exit_code(), 0);
        assert!(error.to_string().lines().count() > 1);
        assert!(report_parse_error_message(&error, invocation_targets_report(&raw_args)).is_none());
    }

    #[test]
    fn another_subcommand_parse_error_keeps_clap_mapping() {
        let args = ["robco", "install", "--unknown"];
        let error = parse_error(&args);
        let raw_args = args.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(error.exit_code(), 2);
        assert!(report_parse_error_message(&error, invocation_targets_report(&raw_args)).is_none());
    }
}
