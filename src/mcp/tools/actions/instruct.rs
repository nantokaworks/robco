//! Send a one-line instruction to a named tmux session on the daemon host.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::tmux;

use super::super::{ToolResult, exec_err, validate_non_blank};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InstructArgs {
    session: String,
    text: String,
}

pub(super) fn instruct(args: InstructArgs) -> ToolResult<Value> {
    validate_non_blank("session", &args.session)?;
    validate_non_blank("text", &args.text)?;
    let text = tmux::single_line(&args.text);
    let server = tmux::TmuxServer::default_server();
    tmux::send_literal_text(&server, &args.session, &text).map_err(exec_err)?;
    tmux::send_keys(&server, &args.session, &["Enter"]).map_err(exec_err)?;
    Ok(json!({ "ok": true, "session": args.session }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::{ToolError, parse_args};

    #[test]
    fn target_and_text_must_not_be_blank() {
        let args = parse_args(Some(json!({"session":"s","text":" "}))).unwrap();
        assert!(matches!(instruct(args), Err(ToolError::InvalidParams(_))));
    }
}
