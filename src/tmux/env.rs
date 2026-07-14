use std::process::Command;

pub fn session_env(session: &str, key: &str) -> Option<String> {
    let output = Command::new("tmux")
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
    use super::parse_session_env;

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
}
