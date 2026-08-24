use serde::Deserialize;
use serde_json::{Value, to_value};

use crate::{
    config::Config, dropr_task_spawn::spawn_dropr_task_in_repo, spawn::spawn_in_repo_with_mode,
};

use super::{ToolResult, exec_err, invalid_params, validate_non_blank};

#[derive(Deserialize)]
pub(super) struct SpawnArgs {
    pub(super) repo: String,
    #[serde(default)]
    pub(super) title: Option<String>,
    pub(super) prompt: Option<String>,
    pub(super) parent_agent_id: Option<String>,
    #[serde(default)]
    pub(super) autonomous: bool,
    #[serde(default)]
    pub(super) dropr_task: Option<String>,
}

pub(super) fn spawn(args: SpawnArgs) -> ToolResult<Value> {
    validate_non_blank("repo", &args.repo)?;
    let config = Config::load().map_err(exec_err)?;
    let extra_args = if args.autonomous {
        config.default_program_autonomous_args()
    } else {
        Vec::new()
    };
    let outcome = match &args.dropr_task {
        Some(task_ref) => {
            validate_non_blank("dropr_task", task_ref)?;
            spawn_dropr_task_in_repo(
                &args.repo,
                task_ref,
                args.title.as_deref(),
                args.prompt.as_deref(),
                None,
                args.parent_agent_id.as_deref(),
                &extra_args,
                args.autonomous,
                &config,
            )
            .map_err(exec_err)?
        }
        None => {
            let title = args
                .title
                .as_deref()
                .ok_or_else(|| invalid_params("title is required when dropr_task is not set"))?;
            validate_non_blank("title", title)?;
            spawn_in_repo_with_mode(
                &args.repo,
                title,
                None,
                args.prompt.as_deref(),
                args.parent_agent_id.as_deref(),
                &extra_args,
                args.autonomous,
                &config,
            )
            .map_err(exec_err)?
        }
    };
    to_value(outcome).map_err(exec_err)
}
