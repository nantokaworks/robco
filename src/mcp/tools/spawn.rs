use serde::Deserialize;
use serde_json::{Value, to_value};

use crate::{config::Config, spawn::spawn_in_repo_with_mode};

use super::{ToolResult, exec_err, validate_non_blank};

#[derive(Deserialize)]
pub(super) struct SpawnArgs {
    repo: String,
    title: String,
    prompt: Option<String>,
    parent_agent_id: Option<String>,
    #[serde(default)]
    autonomous: bool,
}

pub(super) fn spawn(args: SpawnArgs) -> ToolResult<Value> {
    validate_non_blank("repo", &args.repo)?;
    validate_non_blank("title", &args.title)?;
    let config = Config::load().map_err(exec_err)?;
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
    let outcome = spawn_in_repo_with_mode(
        &args.repo,
        &args.title,
        args.prompt.as_deref(),
        args.parent_agent_id.as_deref(),
        &extra_args,
        args.autonomous,
        &config,
    )
    .map_err(exec_err)?;
    to_value(outcome).map_err(exec_err)
}
