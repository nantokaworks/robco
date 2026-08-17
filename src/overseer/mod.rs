use std::path::{Path, PathBuf};

use crate::{Result, config::Config, tmux};

pub mod autonomy;
pub mod command;
pub mod config;
pub(crate) mod config_write;
pub mod daemon;
pub(crate) mod daily;
pub mod discord;
pub mod discord_channels;
pub mod dismissals;
pub mod dispatch;
pub mod exec;
pub mod heartbeat;
pub mod inbox;
pub mod judge;
pub mod ledger;
pub mod logging;
pub mod monitor;
pub mod other_prs;
pub(crate) mod paths;
pub(crate) mod release_pipeline;
pub(crate) mod remedy;
pub(crate) mod repo_watch;
pub mod review;
pub mod runtime_request;
pub(crate) mod session;
pub(crate) mod statefile;
pub mod templates;
pub mod triage;
pub(crate) mod wake;

pub use paths::*;

pub const OVERSEER_AGENT_ID: &str = "overseer";
pub const CONTROL_SESSION_NAME: &str = "@overseer-control";
const LEGACY_OVERSEER_AGENT_ID: &str = "chief";

pub fn control_session_name(prefix: &str) -> String {
    format!("{prefix}{CONTROL_SESSION_NAME}")
}

/// Per-channel Discord ops-agent tmux session name (dropr:371), stable across
/// a channel's turns so a running turn is discoverable and attachable from
/// the TUI. Reserved with the same `@` marker `control_session_name` uses, so
/// `tmux::sanitize_target_part` strips it from a channel id that happens to
/// collide with the literal string, and so the name reads as robco-owned
/// rather than a repo/agent session.
pub fn discord_channel_session_name(prefix: &str, channel_id: &str) -> String {
    format!(
        "{prefix}@discord-{}",
        crate::tmux::sanitize_target_part(channel_id)
    )
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

/// Shown after an operator stops dispatch while leaving the daemon alive.
pub const DISPATCH_STOPPED_HINT: &str =
    "dispatch is off — overseer is stopped; press S here to turn dispatch back on";

/// Shown when the failure circuit has latched dispatch off after repeated worker
/// failures. The circuit disables dispatch and persists it, so the state
/// survives restarts; name the one recovery command, which also clears the
/// consecutive-failure counter.
pub const CIRCUIT_OPEN_HINT: &str = "dispatch circuit is open after repeated worker failures — dispatch stays disabled until you reset it: press [R] here, or run `robco overseer set dispatch on` (re-enables dispatch and clears the failure counter).";

/// Shown while the merge envelope runs under `full_auto`. It names the risks the
/// widened level stops escalating, so the reader can tell a deliberately widened
/// envelope from a gate that is failing to hold.
pub const FULL_AUTO_ENVELOPE_HINT: &str = "autonomy is full_auto — the merge envelope no longer escalates ambiguous requirements, dependency bumps, large diffs, or prod/CI-config changes; only the hard stops (destructive, security, repeated failures, budget, external side effects) still hold.";

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

#[cfg(test)]
mod tests {
    use super::{
        control_session_name, discord_channel_session_name, is_overseer_child,
        migrate_overseer_home,
    };
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
    fn discord_channel_session_name_is_stable_and_disjoint_from_worker_and_control_names() {
        let prefix = "robco_";
        let a = discord_channel_session_name(prefix, "123456789");
        let b = discord_channel_session_name(prefix, "123456789");
        let other = discord_channel_session_name(prefix, "987654321");

        // Deterministic: the same channel id always derives the same session
        // name, so the daemon (spawning it) and the TUI (attaching to it)
        // agree without any shared state beyond the channel id itself.
        assert_eq!(a, b);
        assert_ne!(a, other);
        assert_ne!(a, control_session_name(prefix));
        assert_ne!(a, tmux::session_name(prefix, "123456789", "agent"));

        let reserved_suffix = a.strip_prefix(prefix).unwrap();
        assert!(reserved_suffix.contains('@'));
        assert!(!tmux::sanitize_target_part(reserved_suffix).contains('@'));
    }

    #[test]
    fn discord_channel_session_name_sanitizes_a_hostile_channel_id() {
        let prefix = "robco_";
        let session = discord_channel_session_name(prefix, "../../etc passwd");
        assert!(!session.contains('/'));
        assert!(!session.contains(' '));
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
