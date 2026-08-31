//! Timestamp-bounded suppression of the same derived Inbox rows the TUI shows.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{overseer::dismissals, registry::Registry, ui::inbox};

use super::super::{ToolResult, exec_err, invalid_params, validate_non_blank};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DismissArgs {
    kind: String,
    target_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DismissAllArgs {
    #[serde(default)]
    confirm: bool,
}

pub(super) fn dismiss_one(args: DismissArgs) -> ToolResult<Value> {
    validate_non_blank("kind", &args.kind)?;
    validate_non_blank("target_id", &args.target_id)?;
    let aggregate = current()?;
    let item = aggregate
        .items
        .iter()
        .find(|item| item.kind.code() == args.kind && item.target_id == args.target_id)
        .ok_or_else(|| exec_err("inbox item is no longer listed"))?;
    dismissals::dismiss(
        &[(item.kind.code(), item.target_id.as_str(), item.at)],
        &aggregate.targets,
    )
    .map_err(exec_err)?;
    Ok(json!({
        "ok": true,
        "dismissed": [{ "kind": args.kind, "target_id": args.target_id }],
    }))
}

pub(super) fn dismiss_all(args: DismissAllArgs) -> ToolResult<Value> {
    if !args.confirm {
        return Err(invalid_params(
            "confirm must be true: dismiss-all suppresses every currently listed Inbox item",
        ));
    }
    let aggregate = current()?;
    let targets = aggregate
        .items
        .iter()
        .map(|item| (item.kind.code(), item.target_id.as_str(), item.at))
        .collect::<Vec<_>>();
    dismissals::dismiss(&targets, &aggregate.targets).map_err(exec_err)?;
    Ok(json!({ "ok": true, "dismissed_count": targets.len() }))
}

fn current() -> ToolResult<inbox::Inbox> {
    let registry = Registry::load().map_err(exec_err)?;
    inbox::current(&registry).map_err(exec_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::{ToolError, parse_args};

    #[test]
    fn bulk_dismiss_requires_the_same_confirmation_as_the_tui() {
        let error = dismiss_all(parse_args(Some(json!({}))).unwrap()).unwrap_err();
        assert!(matches!(error, ToolError::InvalidParams(_)));
    }

    #[test]
    fn identity_fields_must_not_be_blank() {
        let args = parse_args(Some(json!({"kind":" ","target_id":"x"}))).unwrap();
        assert!(matches!(
            dismiss_one(args),
            Err(ToolError::InvalidParams(_))
        ));
    }
}
