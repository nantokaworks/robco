//! The local agent-client configuration every worker robco creates is given.
//!
//! Workers report their own turn boundaries back to robco, which the clients
//! only do when their own settings file says so — so the worktree gets one
//! written into it at creation time. This applies to every worker robco
//! creates, not only the ones started `--autonomous`: a `waiting` report is
//! at least as useful for an interactive worker sitting on a prompt the
//! operator is not currently looking at (dropr:532) as it is for an
//! unattended one.

use std::{fs, path::Path};

use serde_json::json;
use toml_edit::{Array, DocumentMut, value};

use crate::Result;

/// Writes the `turn-done` / `waiting` report hooks into `worktree` for
/// `program`, merging into any settings file already there rather than
/// overwriting it. Returns `Ok(true)` when hooks were installed, `Ok(false)`
/// when `program` is not one robco knows how to wire hooks for — a plain
/// `Result<()>` here would let a caller mistake "this program has no hook
/// support" for "hooks were installed"; the bool makes the two cases
/// impossible to conflate at the call site.
pub(super) fn write_report_hooks(worktree: &Path, program: &str) -> Result<bool> {
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
        Ok(true)
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
        Ok(true)
    } else {
        Ok(false)
    }
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
        assert!(write_report_hooks(temp.path(), "claude").unwrap());
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

        assert!(write_report_hooks(temp.path(), &config.default_program_command()).unwrap());

        assert!(temp.path().join(".codex/config.toml").exists());
    }

    /// A program robco has no hook format for still launches cleanly — no
    /// error, and the return value says plainly that nothing was installed
    /// rather than leaving the caller to infer it from an empty worktree.
    #[test]
    fn unsupported_program_installs_nothing_and_does_not_error() {
        let temp = tempfile::tempdir().unwrap();
        let installed = write_report_hooks(temp.path(), "some-other-agent").unwrap();
        assert!(!installed);
        assert!(!temp.path().join(".claude").exists());
        assert!(!temp.path().join(".codex").exists());
    }

    /// An existing `.claude/settings.local.json` — hand-written by the
    /// operator, or left by a previous hook install — is merged into, not
    /// replaced. Its own unrelated keys and hooks must survive.
    #[test]
    fn existing_claude_settings_are_merged_into_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join(".claude");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.local.json"),
            serde_json::to_string(&json!({
                "permissions": {"allow": ["Bash(git status)"]},
                "hooks": {
                    "PreToolUse": [{"hooks": [{"type": "command", "command": "operator-hook"}]}]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(write_report_hooks(temp.path(), "claude").unwrap());

        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("settings.local.json")).unwrap()).unwrap();
        assert_eq!(
            value["permissions"]["allow"][0],
            "Bash(git status)",
            "operator's own settings must survive the merge"
        );
        assert_eq!(
            value["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "operator-hook",
            "a pre-existing hook for a different event must survive the merge"
        );
        assert_eq!(
            value["hooks"]["Stop"][0]["hooks"][0]["command"],
            "robco report --kind turn-done"
        );
        assert_eq!(
            value["hooks"]["Notification"][0]["hooks"][0]["command"],
            "robco report --kind waiting"
        );
    }
}
