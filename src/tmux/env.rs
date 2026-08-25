pub(super) const COLOR_ENV_KEYS: [&str; 5] = [
    "NO_COLOR",
    "FORCE_COLOR",
    "COLORTERM",
    "CLICOLOR",
    "CLICOLOR_FORCE",
];

/// Split color variables into values to mirror and keys to remove.
///
/// A tmux server keeps the environment of the process that started it, so its
/// globals may not reflect the terminal that launched robco. Empty values are
/// still mirrored because their presence can be meaningful to color detection.
pub(super) fn color_env_mirror(
    lookup: impl Fn(&str) -> Option<String>,
) -> (Vec<(&'static str, String)>, Vec<&'static str>) {
    let mut mirror = Vec::new();
    let mut unset = Vec::new();
    for key in COLOR_ENV_KEYS {
        if let Some(value) = lookup(key) {
            mirror.push((key, value));
        } else {
            unset.push(key);
        }
    }
    (mirror, unset)
}

pub fn session_env(server: &super::TmuxServer, session: &str, key: &str) -> Option<String> {
    let output = server
        .command()
        .args(["show-environment", "-t", &format!("={session}:"), key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_session_env(&String::from_utf8_lossy(&output.stdout), key)
}

fn parse_session_env(output: &str, key: &str) -> Option<String> {
    let line = output
        .lines()
        .find(|line| line.starts_with(&format!("{key}=")) || *line == format!("-{key}"))?;
    if line == format!("-{key}") {
        return None;
    }
    line.strip_prefix(&format!("{key}="))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use std::{path::Path, process::Command};

    use super::{COLOR_ENV_KEYS, color_env_mirror, parse_session_env};
    use crate::tmux::{TmuxServer, session::new_session_command_with_lookup};

    fn args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn parses_set_unset_and_missing_values() {
        assert_eq!(
            parse_session_env("KEY=value\n", "KEY").as_deref(),
            Some("value")
        );
        assert_eq!(parse_session_env("-KEY\n", "KEY"), None);
        assert_eq!(parse_session_env("OTHER=value\n", "KEY"), None);
        assert_eq!(parse_session_env("KEY=\n", "KEY"), None);
    }

    #[test]
    fn color_env_mirror_distinguishes_value_empty_and_absent() {
        let (mirror, unset) = color_env_mirror(|key| match key {
            "NO_COLOR" => Some("1".to_string()),
            "FORCE_COLOR" => Some(String::new()),
            _ => None,
        });

        assert_eq!(
            mirror,
            [
                ("NO_COLOR", "1".to_string()),
                ("FORCE_COLOR", String::new())
            ]
        );
        assert_eq!(unset, ["COLORTERM", "CLICOLOR", "CLICOLOR_FORCE"]);
    }

    #[test]
    fn color_env_mirror_preserves_fixed_order_for_each_branch() {
        let (mirror, unset) = color_env_mirror(|key| Some(format!("value-{key}")));
        assert_eq!(
            mirror,
            COLOR_ENV_KEYS.map(|key| (key, format!("value-{key}")))
        );
        assert!(unset.is_empty());

        let (mirror, unset) = color_env_mirror(|_| Some(String::new()));
        assert_eq!(mirror, COLOR_ENV_KEYS.map(|key| (key, String::new())));
        assert!(unset.is_empty());

        let (mirror, unset) = color_env_mirror(|_| None);
        assert!(mirror.is_empty());
        assert_eq!(unset, COLOR_ENV_KEYS);
    }

    #[test]
    fn new_session_command_mirrors_present_and_unsets_absent_color_env() {
        let command = new_session_command_with_lookup(
            &TmuxServer::default_server(),
            "robco_repo_agent",
            Path::new("/repo"),
            "setup && exec codex --flag",
            &[],
            |key| match key {
                "NO_COLOR" => Some("1".to_string()),
                "COLORTERM" => Some(String::new()),
                _ => None,
            },
        );

        assert_eq!(
            args(&command),
            [
                "new-session",
                "-d",
                "-s",
                "robco_repo_agent",
                "-c",
                "/repo",
                "-e",
                "ROBCO_AGENT_ID=",
                "-e",
                "ROBCO_PARENT_AGENT_ID=",
                "-e",
                "CLAUDE_CODE_SESSION_ID=",
                "-e",
                "CLAUDE_CODE_MESSAGING_SOCKET=",
                "-e",
                "CLAUDE_CODE_MESSAGING_TOKEN=",
                "-e",
                "CLAUDE_PID=",
                "-e",
                "CLAUDE_CODE_CHILD_SESSION=",
                "-e",
                "CLAUDECODE=",
                "-e",
                "AI_AGENT=",
                "-e",
                "CODEX_COMPANION_SESSION_ID=",
                "-e",
                "CODEX_COMPANION_TRANSCRIPT_PATH=",
                "-e",
                "NO_COLOR=1",
                "-e",
                "COLORTERM=",
                "unset FORCE_COLOR CLICOLOR CLICOLOR_FORCE; setup && exec codex --flag",
            ]
        );
    }

    #[test]
    fn new_session_command_leaves_program_unwrapped_when_all_color_env_is_present() {
        let command = new_session_command_with_lookup(
            &TmuxServer::default_server(),
            "robco_repo_agent",
            Path::new("/repo"),
            "codex --flag",
            &[],
            |key| Some(format!("value-{key}")),
        );

        let args = args(&command);
        assert_eq!(args.last().map(String::as_str), Some("codex --flag"));
        for key in COLOR_ENV_KEYS {
            assert!(args.contains(&format!("{key}=value-{key}")));
        }
    }

    #[test]
    fn caller_color_env_overrides_mirror_and_is_not_unset() {
        let command = new_session_command_with_lookup(
            &TmuxServer::default_server(),
            "robco_repo_agent",
            Path::new("/repo"),
            "codex",
            &[("NO_COLOR", "caller".to_string())],
            |key| (key == "NO_COLOR").then(|| "launcher".to_string()),
        );

        let args = args(&command);
        assert!(
            args.windows(2)
                .any(|args| args == ["-e", "NO_COLOR=caller"])
        );
        assert!(!args.contains(&"NO_COLOR=launcher".to_string()));
        assert_eq!(
            args.last().map(String::as_str),
            Some("unset FORCE_COLOR COLORTERM CLICOLOR CLICOLOR_FORCE; codex")
        );
    }
}
