use serde::Deserialize;
use serde_json::{Value, to_value};

use crate::{config::Config, spawn::spawn_in_repo_with_mode};

use super::{ToolResult, exec_err, validate_non_blank};

#[derive(Deserialize)]
pub(super) struct SpawnArgs {
    pub(super) repo: String,
    pub(super) title: String,
    pub(super) prompt: Option<String>,
    pub(super) parent_agent_id: Option<String>,
    #[serde(default)]
    pub(super) autonomous: bool,
}

pub(super) fn spawn(args: SpawnArgs) -> ToolResult<Value> {
    validate_non_blank("repo", &args.repo)?;
    validate_non_blank("title", &args.title)?;
    let config = Config::load().map_err(exec_err)?;
    let extra_args = if args.autonomous {
        config.default_program_autonomous_args()
    } else {
        Vec::new()
    };
    let outcome = spawn_in_repo_with_mode(
        &args.repo,
        &args.title,
        None,
        args.prompt.as_deref(),
        args.parent_agent_id.as_deref(),
        &extra_args,
        args.autonomous,
        &config,
    )
    .map_err(exec_err)?;
    to_value(outcome).map_err(exec_err)
}
