//! The local agent-client configuration an autonomous worker is given.
//!
//! Autonomous workers report their own turn boundaries back to robco, which the
//! clients only do when their own settings file says so — so the worktree gets
//! one written into it at spawn time.

use std::{fs, path::Path};

use serde_json::json;
use toml_edit::{Array, DocumentMut, value};

use crate::Result;

pub(super) fn write_autonomous_hooks(worktree: &Path, program: &str) -> Result<()> {
    let executable = program
        .split_whitespace()
        .next()
        .and_then(|executable| Path::new(executable).file_name())
        .and_then(|executable| executable.to_str());
    if executable == Some("claude") {
        let path = worktree.join(".claude/settings.local.json");
        fs::create_dir_all(path.parent().unwrap())?;
        let mut settings = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            json!({})
        };
        add_claude_hook(&mut settings, "Stop", "robco report --kind turn-done");
        add_claude_hook(&mut settings, "Notification", "robco report --kind waiting");
        fs::write(path, serde_json::to_string_pretty(&settings)?)?;
    } else if executable == Some("codex") {
        let path = worktree.join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap())?;
        let mut document = if path.exists() {
            fs::read_to_string(&path)?.parse::<DocumentMut>()?
        } else {
            DocumentMut::new()
        };
        let mut notify = Array::new();
        notify.extend(["sh", "-c", "robco report --kind turn-done"]);
        document["notify"] = value(notify);
        fs::write(path, document.to_string())?;
    }
    Ok(())
}

fn add_claude_hook(settings: &mut serde_json::Value, event: &str, command: &str) {
    if !settings.is_object() {
        *settings = json!({});
    }
    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let event_hooks = hooks
        .as_object_mut()
        .unwrap()
        .entry(event)
        .or_insert_with(|| json!([]));
    if !event_hooks.is_array() {
        *event_hooks = json!([]);
    }
    event_hooks.as_array_mut().unwrap().push(json!({
        "hooks": [{"type": "command", "command": command}]
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn claude_hook_file_contains_both_reports() {
        let temp = tempfile::tempdir().unwrap();
        write_autonomous_hooks(temp.path(), "claude").unwrap();
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(temp.path().join(".claude/settings.local.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            value["hooks"]["Stop"][0]["hooks"][0]["command"],
            "robco report --kind turn-done"
        );
        assert_eq!(
            value["hooks"]["Notification"][0]["hooks"][0]["command"],
            "robco report --kind waiting"
        );
    }

    #[test]
    fn custom_profile_uses_resolved_program_for_hook_format() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            default_program: "codex-autonomous".into(),
            profiles: vec![crate::config::Profile {
                name: "codex-autonomous".into(),
                program: "/usr/local/bin/codex".into(),
                autonomous_args: Vec::new(),
                model: None,
                backend: None,
            }],
            ..Config::default()
        };

        write_autonomous_hooks(temp.path(), &config.default_program_command()).unwrap();

        assert!(temp.path().join(".codex/config.toml").exists());
    }
}
