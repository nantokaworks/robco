use std::path::{Path, PathBuf};

#[path = "spawn/hooks.rs"]
mod hooks;

use serde::Serialize;

use crate::{
    Error, Result, agent,
    config::Config,
    git,
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

/// The branch a spawn for this candidate would collide with, if one is
/// already sitting in the repository — e.g. a previous run's worktree that
/// outlived it (see `crate::overseer::dispatch::worker::spawn_candidate`,
/// dropr:_ord_VtFSIiLgWpgmDAGm). Checked with the exact formula
/// `create_agent_with_launch` uses so this can never disagree with what the
/// real spawn would name the branch. `None` means the spawn is clear to
/// proceed.
pub(crate) fn branch_conflict(
    repo_selector: &str,
    title: &str,
    name_slug: Option<&str>,
    config: &Config,
) -> Result<Option<String>> {
    let registry = Registry::load()?;
    let repo = resolve_repo(&registry, repo_selector)?;
    branch_conflict_for(repo, title, name_slug, config)
}

/// The git-only half of [`branch_conflict`], split out so it can be tested
/// against a throwaway repository without going through `Registry::load`'s
/// on-disk state.
fn branch_conflict_for(
    repo: &RepoNode,
    title: &str,
    name_slug: Option<&str>,
    config: &Config,
) -> Result<Option<String>> {
    let branch = agent::worker_branch_name(config, &repo.name, title, name_slug);
    if git::branch_exists(&repo.path, &branch)? {
        Ok(Some(branch))
    } else {
        Ok(None)
    }
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
#[path = "spawn_tests.rs"]
mod tests;
