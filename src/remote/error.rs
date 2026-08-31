use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum RemoteError {
    #[error("ssh could not connect: {0}")]
    Connect(String),
    #[error("remote robco is not installed: {0}")]
    BinaryMissing(String),
    #[error("remote robco is too old; missing MCP tools: {}", .0.join(", "))]
    MissingTools(Vec<String>),
    #[error("remote connection dropped: {0}")]
    Dropped(String),
    #[error("remote request timed out during {0}")]
    Timeout(String),
    #[error("remote protocol error: {0}")]
    Protocol(String),
    #[error("remote tool {tool} failed: {message}")]
    Tool { tool: String, message: String },
}

impl RemoteError {
    pub(super) fn startup(stderr: &str, fallback: impl Into<String>) -> Self {
        let detail = stderr.trim();
        let detail = if detail.is_empty() {
            fallback.into()
        } else {
            detail.to_string()
        };
        let lower = detail.to_ascii_lowercase();
        if lower.contains("command not found")
            || lower.contains("robco: not found")
            || lower.contains("robco: no such file")
        {
            Self::BinaryMissing(detail)
        } else {
            Self::Connect(detail)
        }
    }
}
