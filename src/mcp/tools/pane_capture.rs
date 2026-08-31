use serde::Deserialize;
use serde_json::Value;

use crate::tmux;

use super::{ToolError, ToolResult, exec_err, validate_non_blank};

pub(super) const CAPTURE_LIMIT_BYTES: usize = 256 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PaneCaptureArgs {
    pub session: String,
    pub width: u16,
    pub height: u16,
    #[serde(default)]
    pub offset: u16,
}

pub(super) fn capture(args: PaneCaptureArgs) -> ToolResult<Value> {
    validate_non_blank("session", &args.session)?;
    if args.width == 0 || args.height == 0 {
        return Err(ToolError::InvalidParams(
            "width and height must be greater than zero".into(),
        ));
    }
    let server = tmux::TmuxServer::default_server();
    let _ = tmux::resize_session(&server, &args.session, args.width, args.height);
    let capture = tmux::capture_scrollback(&server, &args.session, args.offset, args.height)
        .map_err(exec_err)?;
    Ok(Value::String(limit_capture(capture)))
}

fn limit_capture(mut capture: String) -> String {
    if capture.len() <= CAPTURE_LIMIT_BYTES {
        return capture;
    }
    let end = capture
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= CAPTURE_LIMIT_BYTES)
        .last()
        .unwrap_or(0);
    capture.truncate(end);
    capture
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_limit_preserves_utf8() {
        let capture = "x".repeat(CAPTURE_LIMIT_BYTES - 1) + "é";
        let limited = limit_capture(capture);
        assert_eq!(limited.len(), CAPTURE_LIMIT_BYTES - 1);
        assert!(limited.is_char_boundary(limited.len()));
    }

    #[test]
    fn short_capture_is_unchanged() {
        assert_eq!(limit_capture("screen\n".into()), "screen\n");
    }
}
