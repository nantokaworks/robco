use std::path::{Path, PathBuf};

use crate::{Result, config::Config, tmux};

pub mod autonomy;
pub mod command;
pub mod config;
pub mod daemon;
pub mod discord;
pub mod dispatch;
pub mod exec;
pub mod inbox;
pub mod ledger;
pub mod logging;
pub mod monitor;
pub(crate) mod session;
pub mod templates;
pub mod triage;

pub const OVERSEER_AGENT_ID: &str = "overseer";
pub const CONTROL_SESSION_NAME: &str = "@overseer-control";
const LEGACY_OVERSEER_AGENT_ID: &str = "chief";

pub fn control_session_name(prefix: &str) -> String {
    format!("{prefix}{CONTROL_SESSION_NAME}")
}

pub fn ensure_control_session(config: &Config, cwd: &Path) -> Result<String> {
    let session = control_session_name(&config.tmux_session_prefix);
    if !tmux::has_session(&session)?
        && let Err(create_err) =
            tmux::new_session(&session, cwd, &config.default_program_command(), &[])
        && !matches!(tmux::has_session(&session), Ok(true))
    {
        return Err(create_err);
    }
    Ok(session)
}

/// Shown when dispatch is enabled but the Overseer daemon is not running: the
/// toggle is on yet no poll loop consumes ready tasks, so name the two
/// supported ways to start the daemon.
pub const DISPATCH_WITHOUT_DAEMON_HINT: &str = "dispatch is on but the Overseer daemon is not running — no tasks will be dispatched. Start it with `robco overseer run`, or install the always-on service with `robco overseer install-service`.";

pub fn overseer_home() -> Result<PathBuf> {
    migrate_overseer_home(&crate::config::robco_dir()?)
}

fn migrate_overseer_home(robco_dir: &Path) -> Result<PathBuf> {
    let current = robco_dir.join(OVERSEER_AGENT_ID);
    let legacy = robco_dir.join(LEGACY_OVERSEER_AGENT_ID);
    let legacy_exists = legacy.try_exists()?;
    let current_exists = current.try_exists()?;
    if legacy_exists && !current_exists {
        std::fs::rename(&legacy, &current)?;
        for (legacy_name, current_name) in [
            ("chief.pid", "overseer.pid"),
            ("chief.pid.lock", "overseer.pid.lock"),
            ("chief.log", "overseer.log"),
        ] {
            let legacy_file = current.join(legacy_name);
            let current_file = current.join(current_name);
            if legacy_file.try_exists()? && !current_file.try_exists()? {
                std::fs::rename(legacy_file, current_file)?;
            }
        }
    }
    Ok(current)
}

/// Accept persisted workers created before the overseer rename. Remove the
/// legacy value after installations have had enough time to migrate.
pub fn is_overseer_child(parent_agent_id: Option<&str>) -> bool {
    matches!(
        parent_agent_id,
        Some(OVERSEER_AGENT_ID | LEGACY_OVERSEER_AGENT_ID)
    )
}

pub fn ledger_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("ledger.json"))
}

pub fn inbox_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("inbox.jsonl"))
}

pub fn pidfile_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("overseer.pid"))
}

/// True when the Overseer daemon pidfile names a live process. Combined with a
/// fresh heartbeat this is the canonical "daemon is running" signal shared by
/// every status surface (CLI, MCP policy, TUI).
pub fn daemon_pid_alive() -> bool {
    pidfile_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .is_some_and(exec::process_alive)
}

pub fn decision_log_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("decisions.jsonl"))
}

pub fn discord_cursor_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("discord.cursor"))
}

pub fn heartbeat_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("heartbeat"))
}

pub fn snapshots_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("observations.jsonl"))
}

pub fn triage_dir() -> Result<PathBuf> {
    Ok(overseer_home()?.join("triage"))
}

pub fn discord_ops_dir() -> Result<PathBuf> {
    Ok(overseer_home()?.join("discord-ops"))
}

#[cfg(test)]
mod tests {
    use super::{control_session_name, is_overseer_child, migrate_overseer_home};
    use crate::tmux;

    #[test]
    fn control_session_name_is_disjoint_from_worker_names() {
        let prefix = "robco_";
        let control = control_session_name(prefix);

        assert_eq!(control, "robco_@overseer-control");
        assert_ne!(control, tmux::session_name(prefix, "overseer", "control"));

        let reserved_suffix = control.strip_prefix(prefix).unwrap();
        assert!(reserved_suffix.contains('@'));
        assert!(!tmux::sanitize_target_part(reserved_suffix).contains('@'));
    }

    #[test]
    fn overseer_children_include_legacy_parent_id() {
        assert!(is_overseer_child(Some("overseer")));
        assert!(is_overseer_child(Some("chief")));
        assert!(!is_overseer_child(Some("worker")));
        assert!(!is_overseer_child(None));
    }

    #[test]
    fn legacy_state_directory_is_migrated_once() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("chief");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("ledger.json"), "preserved").unwrap();
        std::fs::write(legacy.join("chief.pid"), "123").unwrap();
        std::fs::write(legacy.join("chief.pid.lock"), "").unwrap();
        std::fs::write(legacy.join("chief.log"), "legacy log").unwrap();

        let current = migrate_overseer_home(temp.path()).unwrap();
        assert_eq!(current, temp.path().join("overseer"));
        assert_eq!(
            std::fs::read_to_string(current.join("ledger.json")).unwrap(),
            "preserved"
        );
        assert_eq!(
            std::fs::read_to_string(current.join("overseer.pid")).unwrap(),
            "123"
        );
        assert!(current.join("overseer.pid.lock").exists());
        assert_eq!(
            std::fs::read_to_string(current.join("overseer.log")).unwrap(),
            "legacy log"
        );
        assert!(!current.join("chief.pid").exists());
        assert!(!current.join("chief.pid.lock").exists());
        assert!(!current.join("chief.log").exists());
        assert!(!legacy.exists());

        assert_eq!(migrate_overseer_home(temp.path()).unwrap(), current);
    }
}
