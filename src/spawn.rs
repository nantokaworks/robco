use std::path::{Path, PathBuf};

#[path = "spawn/hooks.rs"]
mod hooks;

use serde::Serialize;

use crate::{
    Error, Result, agent,
    config::Config,
    model::{AgentNode, RepoNode},
    overseer::session::env::SessionEnv,
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
    let launch_env = worker_env(blocked_env, &SessionEnv::resolve(config));
    let program = config.default_program_command();
    let child = agent::create_agent_with_launch(
        &repo,
        title,
        name_slug,
        prompt,
        config,
        parent_agent_id,
        extra_args,
        &launch_env,
        |worktree| {
            if autonomous {
                hooks::write_autonomous_hooks(worktree, &program)?;
            }
            Ok(())
        },
    )?;
    let outcome = SpawnOutcome::from(&child);
    persist_child(&repo.path, child, &outcome)?;
    Ok(outcome)
}

/// The environment a worker's tmux session is launched with.
///
/// Two rules meet here and the order between them is a decision, not an
/// accident. `worker_env_blocklist` blanks *ambient* credentials — names the
/// daemon happens to be carrying that an autonomous worker never asked for —
/// and its default patterns (`*_TOKEN`, `*_API_KEY`) match exactly the names a
/// session credential goes by. The session credential channel
/// ([`SessionEnv`]) is the opposite: an operator writing down what robco's own
/// processes are supposed to run under.
///
/// The explicit declaration wins, for both layers of the channel — the config
/// map and the env file are equally operator-authored. A worker launched under
/// an installed launchd service has the same problem the ephemeral sessions do
/// (no login session to borrow a keychain from), so stripping the one
/// credential the operator configured for it would leave the headless install
/// half-working, and would do so silently. Names the channel does not carry are
/// still blanked.
fn worker_env(blocked: Vec<(String, String)>, session_env: &SessionEnv) -> Vec<(String, String)> {
    let exempt = session_env.names().collect::<Vec<_>>();
    let mut env = blocked
        .into_iter()
        .filter(|(name, _)| !exempt.contains(&name.as_str()))
        .collect::<Vec<_>>();
    env.extend(session_env.pairs());
    env
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

    fn session_env(vars: &[(&str, &str)]) -> SessionEnv {
        SessionEnv::from_config_vars(vars)
    }

    #[test]
    fn configured_session_credential_survives_the_worker_blocklist() {
        let blocked = vec![
            ("CLAUDE_CODE_OAUTH_TOKEN".to_string(), String::new()),
            ("GITHUB_TOKEN".to_string(), String::new()),
        ];

        let env = worker_env(
            blocked,
            &session_env(&[("CLAUDE_CODE_OAUTH_TOKEN", "token")]),
        );

        assert_eq!(
            env,
            vec![
                ("GITHUB_TOKEN".to_string(), String::new()),
                ("CLAUDE_CODE_OAUTH_TOKEN".to_string(), "token".to_string()),
            ]
        );
    }

    #[test]
    fn an_unconfigured_credential_is_still_blanked() {
        let blocked = vec![("AWS_SECRET".to_string(), String::new())];

        let env = worker_env(blocked, &session_env(&[]));

        assert_eq!(env, vec![("AWS_SECRET".to_string(), String::new())]);
    }

    #[test]
    fn interactive_spawns_still_receive_the_session_channel() {
        // `blocked` is empty for a non-autonomous spawn; the channel is not.
        let env = worker_env(Vec::new(), &session_env(&[("ANTHROPIC_API_KEY", "key")]));

        assert_eq!(
            env,
            vec![("ANTHROPIC_API_KEY".to_string(), "key".to_string())]
        );
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
