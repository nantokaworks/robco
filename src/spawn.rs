use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::json;
use toml_edit::{Array, DocumentMut, value};

use crate::{
    Error, Result, agent,
    config::Config,
    model::{AgentNode, RepoNode},
    registry::Registry,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpawnOutcome {
    pub id: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub tmux_session: String,
}

pub fn spawn_in_repo(
    repo_selector: &str,
    title: &str,
    name_slug: Option<&str>,
    prompt: Option<&str>,
    parent_agent_id: Option<&str>,
    extra_args: &[String],
    config: &Config,
) -> Result<SpawnOutcome> {
    spawn_in_repo_with_mode(
        repo_selector,
        title,
        name_slug,
        prompt,
        parent_agent_id,
        extra_args,
        !extra_args.is_empty(),
        config,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_in_repo_with_mode(
    repo_selector: &str,
    title: &str,
    name_slug: Option<&str>,
    prompt: Option<&str>,
    parent_agent_id: Option<&str>,
    extra_args: &[String],
    autonomous: bool,
    config: &Config,
) -> Result<SpawnOutcome> {
    let registry = Registry::load()?;
    let repo = resolve_repo(&registry, repo_selector)?.clone();
    let blocked_env = if autonomous {
        agent::env::autonomous_env(&config.overseer.worker_env_blocklist)
    } else {
        Vec::new()
    };
    let program = config.default_program_command();
    let child = agent::create_agent_with_launch(
        &repo,
        title,
        name_slug,
        prompt,
        config,
        parent_agent_id,
        extra_args,
        &blocked_env,
        |worktree| {
            if autonomous {
                write_autonomous_hooks(worktree, &program)?;
            }
            Ok(())
        },
    )?;
    let outcome = SpawnOutcome::from(&child);
    persist_child(&repo.path, child, &outcome)?;
    Ok(outcome)
}

fn resolve_repo<'a>(registry: &'a Registry, selector: &str) -> Result<&'a RepoNode> {
    let requested = Path::new(selector);
    if requested.is_absolute() {
        let canonical = requested.canonicalize()?;
        return registry
            .repos
            .iter()
            .find(|repo| {
                repo.path
                    .canonicalize()
                    .unwrap_or_else(|_| repo.path.clone())
                    == canonical
            })
            .ok_or_else(|| Error::RepoSelectorNotFound(selector.to_string()));
    }
    let mut matches = registry.repos.iter().filter(|repo| repo.name == selector);
    let repo = matches
        .next()
        .ok_or_else(|| Error::RepoSelectorNotFound(selector.to_string()))?;
    if matches.next().is_some() {
        return Err(Error::RepoSelectorAmbiguous(selector.to_string()));
    }
    Ok(repo)
}

fn persist_child(repo_path: &Path, child: AgentNode, outcome: &SpawnOutcome) -> Result<()> {
    let mut found = false;
    Registry::locked_update(|registry| {
        if let Some(repo) = registry
            .repos
            .iter_mut()
            .find(|repo| repo.path == repo_path)
        {
            repo.agents.push(child);
            found = true;
        }
    })?;
    if found {
        Ok(())
    } else {
        Err(Error::CreatedChildRepoMissing {
            worktree_path: outcome.worktree_path.clone(),
            tmux_session: outcome.tmux_session.clone(),
        })
    }
}

fn write_autonomous_hooks(worktree: &Path, program: &str) -> Result<()> {
    let executable = program
        .split_whitespace()
        .next()
        .and_then(|executable| Path::new(executable).file_name())
        .and_then(|executable| executable.to_str());
    if executable == Some("claude") {
        let path = worktree.join(".claude/settings.local.json");
        fs::create_dir_all(path.parent().unwrap())?;
        let mut settings = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            json!({})
        };
        add_claude_hook(&mut settings, "Stop", "robco report --kind turn-done");
        add_claude_hook(&mut settings, "Notification", "robco report --kind waiting");
        fs::write(path, serde_json::to_string_pretty(&settings)?)?;
    } else if executable == Some("codex") {
        let path = worktree.join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap())?;
        let mut document = if path.exists() {
            fs::read_to_string(&path)?.parse::<DocumentMut>()?
        } else {
            DocumentMut::new()
        };
        let mut notify = Array::new();
        notify.extend(["sh", "-c", "robco report --kind turn-done"]);
        document["notify"] = value(notify);
        fs::write(path, document.to_string())?;
    }
    Ok(())
}

fn add_claude_hook(settings: &mut serde_json::Value, event: &str, command: &str) {
    if !settings.is_object() {
        *settings = json!({});
    }
    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let event_hooks = hooks
        .as_object_mut()
        .unwrap()
        .entry(event)
        .or_insert_with(|| json!([]));
    if !event_hooks.is_array() {
        *event_hooks = json!([]);
    }
    event_hooks.as_array_mut().unwrap().push(json!({
        "hooks": [{"type": "command", "command": command}]
    }));
}

impl From<&AgentNode> for SpawnOutcome {
    fn from(agent: &AgentNode) -> Self {
        Self {
            id: agent.id.clone(),
            branch: agent.branch.clone(),
            worktree_path: agent.worktree_path.clone(),
            tmux_session: agent.tmux_session.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_hook_file_contains_both_reports() {
        let temp = tempfile::tempdir().unwrap();
        write_autonomous_hooks(temp.path(), "claude").unwrap();
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(temp.path().join(".claude/settings.local.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            value["hooks"]["Stop"][0]["hooks"][0]["command"],
            "robco report --kind turn-done"
        );
        assert_eq!(
            value["hooks"]["Notification"][0]["hooks"][0]["command"],
            "robco report --kind waiting"
        );
    }

    #[test]
    fn custom_profile_uses_resolved_program_for_hook_format() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            default_program: "codex-autonomous".into(),
            profiles: vec![crate::config::Profile {
                name: "codex-autonomous".into(),
                program: "/usr/local/bin/codex".into(),
                autonomous_args: Vec::new(),
            }],
            ..Config::default()
        };

        write_autonomous_hooks(temp.path(), &config.default_program_command()).unwrap();

        assert!(temp.path().join(".codex/config.toml").exists());
    }

    #[test]
    fn outcome_copies_agent_shape() {
        let outcome = SpawnOutcome {
            id: "worker".into(),
            branch: "repo/task".into(),
            worktree_path: "/tmp/worktree".into(),
            tmux_session: "robco-task".into(),
        };
        let value = serde_json::to_value(outcome).unwrap();
        assert_eq!(value["id"], "worker");
        assert_eq!(value["branch"], "repo/task");
        assert_eq!(value["worktree_path"], "/tmp/worktree");
        assert_eq!(value["tmux_session"], "robco-task");
    }
}
