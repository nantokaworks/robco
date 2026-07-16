use std::{collections::BTreeSet, process::Command};

use crate::config::{ENV_AGENT_ID, ENV_PARENT_AGENT_ID};

pub struct RecoveredIdentity {
    pub id: String,
    pub parent_agent_id: Option<String>,
}

pub fn agent_env(id: &str, parent_agent_id: Option<&str>) -> Vec<(&'static str, String)> {
    let mut env = vec![(ENV_AGENT_ID, id.to_string())];
    if let Some(parent) = parent_agent_id.filter(|parent| !parent.is_empty()) {
        env.push((ENV_PARENT_AGENT_ID, parent.to_string()));
    }
    env
}

pub(crate) fn launch_command(
    program: &str,
    initial_prompt: Option<&str>,
    extra_args: &[String],
) -> String {
    let mut command = program.to_string();
    for arg in extra_args {
        command.push(' ');
        command.push_str(&shell_quote(arg));
    }
    if let Some(prompt) = initial_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        command.push(' ');
        command.push_str(&shell_quote(prompt));
    }
    command
}

pub(crate) fn autonomous_env(patterns: &[String]) -> Vec<(String, String)> {
    autonomous_env_with_name_sources(
        patterns,
        || std::env::vars().map(|(name, _)| name),
        tmux_global_env_names,
    )
}

fn autonomous_env_with_name_sources<I, J>(
    patterns: &[String],
    process_names: impl FnOnce() -> I,
    global_names: impl FnOnce() -> J,
) -> Vec<(String, String)>
where
    I: IntoIterator<Item = String>,
    J: IntoIterator<Item = String>,
{
    process_names()
        .into_iter()
        .chain(global_names())
        .filter(|name| patterns.iter().any(|pattern| glob_matches(pattern, name)))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|name| (name, String::new()))
        .collect()
}

fn tmux_global_env_names() -> Vec<String> {
    let output = Command::new("tmux")
        .args(["show-environment", "-g"])
        .output()
        .map(|output| (output.status.success(), output.stdout));
    tmux_global_env_names_from_result(output)
}

fn tmux_global_env_names_from_result(output: std::io::Result<(bool, Vec<u8>)>) -> Vec<String> {
    match output {
        Ok((true, stdout)) => parse_tmux_global_env_names(&String::from_utf8_lossy(&stdout)),
        Ok((false, _)) | Err(_) => Vec::new(),
    }
}

fn parse_tmux_global_env_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            line.strip_prefix('-')
                .or_else(|| line.split_once('=').map(|(name, _)| name))
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        })
        .collect()
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v, mut star, mut retry) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && pattern[p] == value[v] {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = v;
        } else if let Some(position) = star {
            retry += 1;
            v = retry;
            p = position + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_identity_with_and_without_parent() {
        assert_eq!(
            agent_env("child", None),
            vec![(ENV_AGENT_ID, "child".into())]
        );
        assert_eq!(
            agent_env("child", Some("parent")),
            vec![
                (ENV_AGENT_ID, "child".into()),
                (ENV_PARENT_AGENT_ID, "parent".into())
            ]
        );
        assert_eq!(agent_env("child", Some("")), agent_env("child", None));
    }

    #[test]
    fn launch_command_quotes_extra_args_before_prompt() {
        assert_eq!(
            launch_command("codex", Some("do it"), &["--flag".into(), "a'b".into()]),
            "codex '--flag' 'a'\\''b' 'do it'"
        );
    }

    #[test]
    fn blocklist_globs_match_expected_names() {
        assert!(glob_matches("AWS_*", "AWS_PROFILE"));
        assert!(glob_matches("*_TOKEN", "GITHUB_TOKEN"));
        assert!(glob_matches("*_API_KEY", "OPENAI_API_KEY"));
        assert!(glob_matches("A*B", "AXBYB"));
        assert!(!glob_matches("*_TOKEN", "TOKEN_CACHE"));
    }

    #[test]
    fn autonomous_env_neutralizes_name_only_in_tmux_global_env() {
        let env = autonomous_env_with_name_sources(&["*_TOKEN".into()], Vec::<String>::new, || {
            vec!["GITHUB_TOKEN".into(), "PATH".into()]
        });

        assert_eq!(env, vec![("GITHUB_TOKEN".into(), String::new())]);
    }

    #[test]
    fn parses_tmux_global_environment_output() {
        assert_eq!(
            parse_tmux_global_env_names("KEY=value\n-REMOVED\nEMPTY=\n"),
            vec!["KEY", "REMOVED", "EMPTY"]
        );
        assert!(parse_tmux_global_env_names("").is_empty());
        assert!(parse_tmux_global_env_names("MALFORMED\n=missing-name\n-\n").is_empty());
    }

    #[test]
    fn tmux_global_environment_failure_is_empty() {
        let unavailable = Err(std::io::Error::new(std::io::ErrorKind::NotFound, "tmux"));
        assert!(tmux_global_env_names_from_result(unavailable).is_empty());
        assert!(tmux_global_env_names_from_result(Ok((false, b"KEY=value".to_vec()))).is_empty());
    }
}
