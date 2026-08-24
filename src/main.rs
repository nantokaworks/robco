mod agent;
mod browser;
mod cli;
mod clone;
mod config;
mod discover;
mod dropr;
mod dropr_task_spawn;
mod error;
mod exec;
mod git;
mod guard;
mod loading;
mod locale;
mod mcp;
mod model;
mod new_agent;
mod notify;
mod openclaw;
mod overseer;
mod pr;
mod registry;
mod rename;
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

pub use error::{Error, Result};

#[tokio::main]
async fn main() -> ExitCode {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let legacy_overseer = cli::rewrite_legacy_overseer(&raw_args);
    let parse_args = legacy_overseer.as_deref().unwrap_or(&raw_args);
    let args = match Args::try_parse_from(parse_args) {
        Ok(args) => args,
        Err(err) => {
            if err.kind() == ErrorKind::DisplayVersion {
                version::print();
                return ExitCode::SUCCESS;
            }
            if let Some(message) =
                cli::report_parse_error_message(&err, cli::invocation_targets_report(parse_args))
            {
                eprintln!("{message}");
                return ExitCode::from(3);
            }
            err.exit();
        }
    };
    if let Some(rewritten) = &legacy_overseer {
        let new_form = rewritten[1..]
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("robco: `overseer` is retired; run `robco {new_form}` next time");
    }
    if matches!(&args.command, Some(Command::Version)) {
        version::print();
        return ExitCode::SUCCESS;
    }
    if let Some(Command::Report(report_args)) = &args.command {
        return run_report(report_args);
    }
    // Handled here, before the config load, for the same reason `report` is:
    // it runs on every shell command a worker issues, so it must stay fast,
    // and a config robco cannot read must not turn the guard off.
    if let Some(Command::Guard(guard_args)) = &args.command {
        return guard::run(guard_args.kind);
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
        if matches!(command, Command::Daemon) {
            return overseer::daemon::run_daemon().await;
        }
        return run_command(command, &config, args.launch_dir.as_deref());
    }

    let indicator = loading::Indicator::start("Scanning repositories...");
    let roots = effective_roots(&config.repos_root, args.launch_dir.as_deref());
    let discovered = discover::discover_with_overlay(&roots, &config, &indicator);

    let registry = Registry::locked_update(|registry| {
        agent::normalize_adopted_titles(&mut registry.repos, &config);
        registry.merge_discovered(discovered);
    })?;
    let ephemeral_root = args
        .launch_dir
        .map(|path| path.canonicalize().unwrap_or(path));
    indicator.finish();
    ui::run(registry, config, ephemeral_root)
}

fn run_command(
    command: Command,
    config: &Config,
    ephemeral_root: Option<&std::path::Path>,
) -> Result<()> {
    match command {
        Command::Add(args) => {
            let path = clone::clone_and_register(
                &args.url,
                &config.repos_root,
                args.branch.as_deref(),
                args.name.as_deref(),
            )?;
            println!("added {}", path.display());
        }
        Command::Config(args) => overseer::command::run_config(args.command)?,
        Command::Daemon => unreachable!("daemon is handled before sync commands"),
        Command::Debug => {
            println!("config: {}", config::config_file_path()?.display());
            println!("state: {}", config::state_path()?.display());
            println!("worktrees: {}", config.worktree_root.display());
            println!("repos: {}", config.repos_root.display());
            println!("program: {}", config.default_program_command());
            println!("dropr_overlay: {}", config.dropr_overlay);
            println!("auto_accept: {}", config.auto_accept);
        }
        Command::Decisions(args) => overseer::command::run_decisions(args.command)?,
        Command::Guard(_) => unreachable!("guard is handled before config loading"),
        Command::Inbox(args) => overseer::command::run_inbox(args.command)?,
        Command::Install(args) => setup::install_command(&args)?,
        Command::List(args) => {
            let roots = effective_roots(&config.repos_root, args.dir.as_deref().or(ephemeral_root));
            list_repositories(&roots, config)?;
        }
        Command::McpStdio => unreachable!("mcp-stdio is handled before sync commands"),
        Command::New(args) => new_agent::run(args, config)?,
        Command::Panic => overseer::command::panic_stop()?,
        Command::Rename(args) => run_rename(&args)?,
        Command::Report(_) => unreachable!("report is handled before config loading"),
        Command::Restart => overseer::command::restart()?,
        Command::Service(args) => overseer::command::run_service(args.command)?,
        Command::Spawn(args) => {
            let outcome = spawn::run_spawn_command(args, config)?;
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
        Command::Start => overseer::command::start()?,
        Command::Status(args) => overseer::command::status(config, args.debug)?,
        Command::Stop => overseer::command::stop()?,
        Command::Uninstall(args) => setup::uninstall(&args)?,
        Command::Version => unreachable!("version is handled before config loading"),
    }
    Ok(())
}

fn run_rename(args: &cli::RenameArgs) -> Result<()> {
    let registry = Registry::locked_load()?;
    let repo = spawn::resolve_repo(&registry, &args.repo)?;
    if !repo.agents.is_empty() {
        return Err(Error::Command {
            context: "repo rename",
            stderr: format!(
                "{} has {} agent(s) attached; remove them first",
                repo.name,
                repo.agents.len()
            ),
        });
    }
    let old_path = repo.path.clone();

    let outcome = rename::rename_repo_dir(&old_path, &args.name)?;
    let mut applied = false;
    Registry::locked_update(|registry| {
        applied = rename::apply_rename(registry, &old_path, &outcome.new_path, &args.name);
    })?;

    println!("renamed to {}", outcome.new_path.display());
    if !applied {
        println!(
            "warning: {} moved on disk, but was no longer in robco's registry to update; \
             run robco again to re-discover it",
            outcome.new_path.display()
        );
    }
    if !outcome.unrepaired_worktrees.is_empty() {
        println!("warning: some worktrees still need manual repair:");
        for (worktree, error) in &outcome.unrepaired_worktrees {
            println!("  {}: {error}", worktree.display());
        }
        println!(
            "run: git -C {} worktree repair <worktree-path>",
            outcome.new_path.display()
        );
    }
    Ok(())
}

fn effective_roots(
    repos_root: &std::path::Path,
    ephemeral_root: Option<&std::path::Path>,
) -> Vec<PathBuf> {
    let mut roots = vec![repos_root.to_path_buf()];
    if let Some(root) = ephemeral_root
        && root != repos_root
    {
        roots.push(root.to_path_buf());
    }
    roots
}

fn list_repositories(roots: &[PathBuf], config: &Config) -> Result<()> {
    let indicator = loading::Indicator::start("Scanning repositories...");
    let discovered = discover::discover_with_overlay(roots, config, &indicator);
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
