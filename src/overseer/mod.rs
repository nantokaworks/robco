use std::path::{Path, PathBuf};

use crate::{Result, config::Config, tmux};

pub mod command;
pub mod config;
pub mod daemon;
pub(crate) mod daily;
pub mod discord;
pub mod discord_channels;
pub mod dismissals;
pub mod dispatch;
pub mod exec;
pub mod heartbeat;
pub mod inbox;
pub mod ledger;
pub mod logging;
pub mod monitor;
pub mod other_prs;
pub(crate) mod paths;
pub(crate) mod remedy;
pub(crate) mod repo_lookup;
pub(crate) mod repo_watch;
pub mod review;
pub mod row_summaries;
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
    if !tmux::has_session(&config.tmux_server, &session)?
        && let Err(create_err) = tmux::new_session(
            &config.tmux_server,
            &session,
            cwd,
            &config.default_program_command(),
            &[],
        )
        && !matches!(tmux::has_session(&config.tmux_server, &session), Ok(true))
    {
        return Err(create_err);
    }
    Ok(session)
}

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

/// True when `parent_agent_id` names another worker in the registry — a
/// subagent `robco new` spawned from inside a running worker session,
/// instead of a top-level worker enrolled with the Overseer at creation.
///
/// dropr:521 dropped ownership (`is_overseer_child`) as the gate on which
/// worktrees the Overseer daemon will land, list, or kill: every worker in
/// the registry counts now, no matter how or when it was created. The one
/// worker that still must not count on its own is a worker's own child —
/// its parent worker is already the thing whose pull request matters, so
/// the subagent's worktree stays out of the ledger and every report.
pub fn is_worker_subagent(
    parent_agent_id: Option<&str>,
    registry: &crate::registry::Registry,
) -> bool {
    let Some(parent) = parent_agent_id else {
        return false;
    };
    registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .any(|agent| agent.id == parent)
}

#[cfg(test)]
mod tests {
    use super::{
        OVERSEER_AGENT_ID, control_session_name, discord_channel_session_name, is_overseer_child,
        is_worker_subagent, migrate_overseer_home,
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

    fn registry_with_worker(id: &str, parent: Option<&str>) -> crate::registry::Registry {
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "repos": [{
                "path": "/repo",
                "name": "repo",
                "remote_url": null,
                "agents": [{
                    "id": id,
                    "parent_agent_id": parent,
                    "title": "#1",
                    "worktree_path": "/repo/worker",
                    "branch": "task-1",
                    "base_commit": "",
                    "program": "codex",
                    "tmux_session": "robco_repo_task-1",
                    "created_at": "2026-07-18T00:00:00+09:00",
                    "updated_at": "2026-07-18T00:00:00+09:00"
                }]
            }]
        }))
        .unwrap()
    }

    #[test]
    fn a_worker_whose_parent_is_the_overseer_is_not_a_subagent() {
        let registry = registry_with_worker("worker", Some(OVERSEER_AGENT_ID));
        assert!(!is_worker_subagent(Some(OVERSEER_AGENT_ID), &registry));
    }

    #[test]
    fn a_worker_with_no_parent_is_not_a_subagent() {
        let registry = registry_with_worker("worker", None);
        assert!(!is_worker_subagent(None, &registry));
    }

    #[test]
    fn a_worker_whose_parent_is_a_stranger_id_is_not_a_subagent() {
        // The parent string does not name any agent the registry still
        // lists — a foreign or stale id, not a live worker to nest under.
        let registry = registry_with_worker("worker", Some("some-other-id"));
        assert!(!is_worker_subagent(Some("gone"), &registry));
    }

    #[test]
    fn a_worker_whose_parent_is_another_registry_worker_is_a_subagent() {
        let mut registry = registry_with_worker("parent-worker", Some(OVERSEER_AGENT_ID));
        let mut child = registry.repos[0].agents[0].clone();
        child.id = "child-worker".into();
        child.parent_agent_id = Some("parent-worker".into());
        registry.repos[0].agents.push(child);
        assert!(is_worker_subagent(Some("parent-worker"), &registry));
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
