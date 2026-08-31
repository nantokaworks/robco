//! Actions on a registered repository's primary checkout and chat session.
//! Clearing chat is irreversible, so its confirmation gate signals intent;
//! it does not authenticate the caller.

use std::path::Path;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent, config::Config, git, model::Status, registry::Registry, rename,
    status::WatchStatusState, tmux,
};

use super::super::{ToolResult, exec_err, invalid_params, validate_non_blank};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RepoArgs {
    repo_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ClearArgs {
    repo_path: String,
    #[serde(default)]
    confirm: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RenameArgs {
    repo_path: String,
    new_name: String,
}

pub(super) fn checkout_main(args: RepoArgs) -> ToolResult<Value> {
    let registry = load_and_validate(&args.repo_path)?;
    let repo = find_repo(&registry, &args.repo_path)?;
    let branch = git::default_branch(&repo.path)
        .map_err(exec_err)?
        .ok_or_else(|| exec_err("default branch could not be resolved"))?;
    if !git::worktree_is_clean(&repo.path).map_err(exec_err)? {
        return Err(exec_err(format!(
            "commit or clean untracked changes before checking out {branch}"
        )));
    }
    git::checkout(&repo.path, &branch).map_err(exec_err)?;
    Ok(json!({ "ok": true, "repo_path": repo.path, "branch": branch }))
}

pub(super) fn clear_chat(args: ClearArgs) -> ToolResult<Value> {
    validate_non_blank("repo_path", &args.repo_path)?;
    if !args.confirm {
        return Err(invalid_params(
            "confirm must be true: robco_repo_clear_chat discards chat history",
        ));
    }
    let registry = Registry::load().map_err(exec_err)?;
    let repo = find_repo(&registry, &args.repo_path)?;
    let config = Config::load().map_err(exec_err)?;
    let command = config.default_program_clear_command().ok_or_else(|| {
        exec_err(format!(
            "no clear command configured for {}",
            config.default_program_command()
        ))
    })?;
    let session = agent::repo_claude_session_name(&config.tmux_session_prefix, repo);
    if !tmux::has_session(&config.tmux_server, &session).map_err(exec_err)? {
        return Err(exec_err("no live chat session to clear"));
    }
    let status =
        crate::status::classify_session_status(&session, None, &mut WatchStatusState::default());
    if !matches!(status, Some(Status::Idle) | Some(Status::Done)) {
        return Err(exec_err(
            "chat session is busy — wait for it to finish before clearing",
        ));
    }
    tmux::send_literal_text(&config.tmux_server, &session, &command).map_err(exec_err)?;
    tmux::send_keys(&config.tmux_server, &session, &["Enter"]).map_err(exec_err)?;
    Ok(json!({ "ok": true, "repo_path": repo.path, "session": session }))
}

pub(super) fn rename(args: RenameArgs) -> ToolResult<Value> {
    validate_non_blank("new_name", &args.new_name)?;
    let registry = load_and_validate(&args.repo_path)?;
    let repo = find_repo(&registry, &args.repo_path)?;
    if !repo.agents.is_empty() {
        return Err(exec_err("remove agents first"));
    }
    let old_path = repo.path.clone();
    let outcome = rename::rename_repo_dir(&old_path, &args.new_name).map_err(exec_err)?;
    let new_path = outcome.new_path.clone();
    let new_name = args.new_name.clone();
    let mut applied = false;
    Registry::locked_update(|registry| {
        applied = rename::apply_rename(registry, &old_path, &new_path, &new_name);
    })
    .map_err(exec_err)?;
    if !applied {
        return Err(exec_err(format!(
            "renamed directory to {}, but it was no longer registered",
            new_path.display()
        )));
    }
    let unrepaired = outcome
        .unrepaired_worktrees
        .iter()
        .map(|(path, error)| json!({ "path": path, "error": error }))
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": unrepaired.is_empty(),
        "old_path": old_path,
        "new_path": new_path,
        "new_name": args.new_name,
        "unrepaired_worktrees": unrepaired,
    }))
}

fn load_and_validate(repo_path: &str) -> ToolResult<Registry> {
    validate_non_blank("repo_path", repo_path)?;
    Registry::load().map_err(exec_err)
}

fn find_repo<'a>(
    registry: &'a Registry,
    repo_path: &str,
) -> ToolResult<&'a crate::model::RepoNode> {
    registry
        .repos
        .iter()
        .find(|repo| repo.path == Path::new(repo_path))
        .ok_or_else(|| exec_err(format!("repo_path not found: {repo_path}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::{ToolError, parse_args};

    #[test]
    fn clear_requires_confirmation_before_loading_state() {
        let error = clear_chat(parse_args(Some(json!({"repo_path":"/r"}))).unwrap()).unwrap_err();
        assert!(matches!(error, ToolError::InvalidParams(_)));
    }

    #[test]
    fn repo_arguments_are_strict() {
        assert!(parse_args::<RepoArgs>(Some(json!({"repo_path":"/r","extra":1}))).is_err());
    }
}
