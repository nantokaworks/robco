//! The launchd job definition, and the environment it carries.
//!
//! A launchd agent inherits nothing from the user's login shell — the plist's
//! `EnvironmentVariables` dictionary is the whole of its environment, which is
//! why `PATH` has to be materialised here rather than assumed. The same is true
//! of the credential the daemon's sessions need, so the installer writes the
//! configured `overseer.session_env` into the same dictionary.
//!
//! What it deliberately does *not* write is the env file. `~/.robco/env` is read
//! by the daemon at spawn time, so a rotated token takes effect on the next
//! session instead of requiring the service to be reinstalled and reloaded —
//! and the secret stays in one file whose permissions the operator controls.
//! Nothing here reaches for the keychain: an agent cannot unlock the login
//! session's keychain, which is the failure this whole channel exists to route
//! around.

use std::{collections::BTreeMap, path::Path};

pub(super) const PLIST_MODE: u32 = 0o600;

/// Build the job definition. `session_env` is written verbatim beside `PATH`;
/// callers are responsible for having resolved it from the config.
pub(super) fn render(
    executable: &Path,
    path_env: &str,
    log: &Path,
    session_env: &BTreeMap<String, String>,
) -> String {
    let mut environment = format!("<key>PATH</key><string>{}</string>", xml(path_env));
    for (name, value) in session_env {
        environment.push_str(&format!(
            "<key>{}</key><string>{}</string>",
            xml(name),
            xml(value)
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.robco.overseer</string>
<key>ProgramArguments</key><array><string>{}</string><string>daemon</string></array>
<key>EnvironmentVariables</key><dict>{environment}</dict>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string>
</dict></plist>
"#,
        xml(&executable.to_string_lossy()),
        xml(&log.to_string_lossy()),
        xml(&log.to_string_lossy())
    )
}

/// PATH for the launchd service. launchd agents get a bare system PATH
/// (`/usr/bin:/bin:/usr/sbin:/sbin`) that hides the tools the daemon shells
/// out to (dropr, tmux, git, the agent CLI). Start from the install-time
/// PATH and ensure the common tool dirs and system dirs are present.
pub(super) fn path_env(current: Option<&str>, home: &Path) -> String {
    let mut path = current.unwrap_or("").to_string();
    let required = [
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        home.join(".local/bin").to_string_lossy().into_owned(),
        "/usr/bin".to_string(),
        "/bin".to_string(),
        "/usr/sbin".to_string(),
        "/sbin".to_string(),
    ];
    for dir in required {
        if !path.split(':').any(|entry| entry == dir) {
            if !path.is_empty() {
                path.push(':');
            }
            path.push_str(&dir);
        }
    }
    path
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn session_env(vars: &[(&str, &str)]) -> BTreeMap<String, String> {
        vars.iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn program_arguments_invoke_the_flat_daemon_subcommand() {
        // The launchd label stays `com.robco.overseer` — only the argv the
        // service invokes moves off the retired `overseer run` spelling.
        let plist = render(
            &PathBuf::from("/usr/local/bin/robco"),
            "/usr/bin",
            &PathBuf::from("/log"),
            &BTreeMap::new(),
        );

        assert!(plist.contains("<key>Label</key><string>com.robco.overseer</string>"));
        assert!(plist.contains(
            "<key>ProgramArguments</key><array><string>/usr/local/bin/robco</string><string>daemon</string></array>"
        ));
    }

    #[test]
    fn session_env_is_written_beside_path() {
        let plist = render(
            &PathBuf::from("/usr/local/bin/robco"),
            "/usr/bin:/bin",
            &PathBuf::from("/Users/me/.robco/overseer/overseer.log"),
            &session_env(&[("CLAUDE_CODE_OAUTH_TOKEN", "sk-token")]),
        );

        assert!(plist.contains(
            "<key>EnvironmentVariables</key><dict><key>PATH</key><string>/usr/bin:/bin</string><key>CLAUDE_CODE_OAUTH_TOKEN</key><string>sk-token</string></dict>"
        ));
    }

    #[test]
    fn an_empty_channel_leaves_the_definition_as_it_was() {
        let plist = render(
            &PathBuf::from("/usr/local/bin/robco"),
            "/usr/bin",
            &PathBuf::from("/log"),
            &BTreeMap::new(),
        );

        assert!(plist.contains(
            "<key>EnvironmentVariables</key><dict><key>PATH</key><string>/usr/bin</string></dict>"
        ));
    }

    #[test]
    fn markup_in_a_value_cannot_break_out_of_its_element() {
        let plist = render(
            &PathBuf::from("/bin/robco"),
            "/usr/bin",
            &PathBuf::from("/log"),
            &session_env(&[("A&B", "</string><key>Injected</key><string>x")]),
        );

        assert!(!plist.contains("<key>Injected</key>"));
        assert!(plist.contains("<key>A&amp;B</key>"));
    }

    #[test]
    fn path_env_appends_missing_tool_dirs() {
        assert_eq!(
            path_env(
                Some("/usr/bin:/bin:/usr/sbin:/sbin"),
                Path::new("/Users/me")
            ),
            "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin:/Users/me/.local/bin"
        );
    }

    #[test]
    fn path_env_keeps_existing_order() {
        assert_eq!(
            path_env(
                Some("/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:/Users/me/.local/bin"),
                Path::new("/Users/me")
            ),
            "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:/Users/me/.local/bin:/usr/local/bin"
        );
    }

    #[test]
    fn path_env_builds_from_missing_path() {
        assert_eq!(
            path_env(None, Path::new("/Users/me")),
            "/opt/homebrew/bin:/usr/local/bin:/Users/me/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }
}
