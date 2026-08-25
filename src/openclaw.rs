use std::{
    io::Write,
    process::{Command, Stdio},
    thread,
};

use serde::{Deserialize, Serialize};

use crate::{notify::WatchTarget, tmux};

const PROMPT_LINES: usize = 20;
const CURL_MAX_TIME_SECONDS: &str = "5";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum OpenClawEndpoint {
    #[default]
    Agent,
    Wake,
}

impl OpenClawEndpoint {
    pub fn path_suffix(self) -> &'static str {
        match self {
            Self::Agent => "/hooks/agent",
            Self::Wake => "/hooks/wake",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OpenClawConfig {
    pub enabled: bool,
    pub webhook_url: String,
    pub token: String,
    #[serde(default)]
    pub endpoint: OpenClawEndpoint,
}

impl OpenClawConfig {
    pub fn is_active(&self) -> bool {
        self.enabled && !self.webhook_url.trim().is_empty()
    }

    pub fn endpoint_url(&self) -> String {
        format!(
            "{}{}",
            self.webhook_url.trim().trim_end_matches('/'),
            self.endpoint.path_suffix()
        )
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct TransitionPayload {
    repo: String,
    agent_id: String,
    agent: String,
    status: String,
    prompt: String,
    options: Vec<String>,
}

impl TransitionPayload {
    fn from_target(target: &WatchTarget) -> Self {
        Self {
            repo: target.repo.clone(),
            agent_id: target.agent_id.clone(),
            agent: target.label.clone(),
            status: target.status.badge().to_string(),
            prompt: prompt_tail(&target.tmux_session),
            options: Vec::new(),
        }
    }
}

pub fn post_transition(cfg: &OpenClawConfig, target: &WatchTarget) {
    if !cfg.is_active() {
        return;
    }

    let cfg = cfg.clone();
    let target = target.clone();
    let _ = thread::Builder::new()
        .name("openclaw-transition".to_string())
        .spawn(move || post_transition_blocking(cfg, target));
}

fn post_transition_blocking(cfg: OpenClawConfig, target: WatchTarget) {
    let payload = TransitionPayload::from_target(&target);
    let Ok(body) = serde_json::to_string(&payload) else {
        return;
    };

    let config = curl_config(&cfg, &body);
    let Ok(mut child) = Command::new("curl")
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(config.as_bytes());
    }
    let _ = child.wait();
}

fn curl_config(cfg: &OpenClawConfig, body: &str) -> String {
    let bearer = format!("Authorization: Bearer {}", cfg.token);
    [
        config_line("request", "POST"),
        config_line("url", &cfg.endpoint_url()),
        config_line("header", "Content-Type: application/json"),
        config_line("header", &bearer),
        config_line("data", body),
        config_line("max-time", CURL_MAX_TIME_SECONDS),
        "fail".to_string(),
        "silent".to_string(),
        "show-error".to_string(),
    ]
    .join("\n")
}

fn config_line(key: &str, value: &str) -> String {
    format!("{key} = \"{}\"", curl_config_escape(value))
}

fn curl_config_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn prompt_tail(session: &str) -> String {
    let server = tmux::TmuxServer::default_server();
    tmux::capture_plain(&server, session)
        .or_else(|_| tmux::capture_text(&server, session))
        .map(|capture| tail_non_empty_lines(&capture, PROMPT_LINES))
        .unwrap_or_default()
}

fn tail_non_empty_lines(text: &str, limit: usize) -> String {
    let mut lines = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(limit)
        .collect::<Vec<_>>();
    lines.reverse();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model::Status;

    #[test]
    fn config_default_is_disabled() {
        let cfg = OpenClawConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.webhook_url, "");
        assert_eq!(cfg.token, "");
        assert_eq!(cfg.endpoint, OpenClawEndpoint::Agent);
        assert!(!cfg.is_active());
    }

    #[test]
    fn is_active_requires_enabled_and_non_blank_url() {
        let cfg = OpenClawConfig {
            enabled: true,
            webhook_url: "   ".to_string(),
            ..OpenClawConfig::default()
        };
        assert!(!cfg.is_active());

        let cfg = OpenClawConfig {
            webhook_url: "https://openclaw.example".to_string(),
            ..cfg
        };
        assert!(cfg.is_active());
    }

    #[test]
    fn endpoint_suffix_and_url_join_do_not_double_slash() {
        let cfg = OpenClawConfig {
            enabled: true,
            webhook_url: "https://openclaw.example/".to_string(),
            endpoint: OpenClawEndpoint::Agent,
            ..OpenClawConfig::default()
        };
        assert_eq!(OpenClawEndpoint::Agent.path_suffix(), "/hooks/agent");
        assert_eq!(cfg.endpoint_url(), "https://openclaw.example/hooks/agent");

        let cfg = OpenClawConfig {
            endpoint: OpenClawEndpoint::Wake,
            ..cfg
        };
        assert_eq!(OpenClawEndpoint::Wake.path_suffix(), "/hooks/wake");
        assert_eq!(cfg.endpoint_url(), "https://openclaw.example/hooks/wake");
    }

    #[test]
    fn payload_serializes_expected_shape() {
        let payload = TransitionPayload {
            repo: "robco".to_string(),
            agent_id: "agent-1".to_string(),
            agent: "implementer".to_string(),
            status: Status::Waiting.badge().to_string(),
            prompt: "Choose one".to_string(),
            options: Vec::new(),
        };

        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            json!({
                "repo": "robco",
                "agent_id": "agent-1",
                "agent": "implementer",
                "status": "wait",
                "prompt": "Choose one",
                "options": []
            })
        );
    }

    #[test]
    fn curl_config_escape_handles_quotes_and_backslashes() {
        assert_eq!(curl_config_escape(r#"a\b"c"#), r#"a\\b\"c"#);
    }
}
