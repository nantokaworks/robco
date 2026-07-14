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

pub(super) fn launch_command(program: &str, initial_prompt: Option<&str>) -> String {
    match initial_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        Some(prompt) => format!("{program} {}", shell_quote(prompt)),
        None => program.to_string(),
    }
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
}
