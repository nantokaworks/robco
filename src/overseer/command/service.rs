//! Everything below `install_service` is launchd, and launchd is macOS. The
//! consumers in `setup::wizard::steps_service` are gated the same way, so on
//! any other target these items would compile only to sit unused.

#[cfg(target_os = "macos")]
use std::{fs, process::Command, time::Duration};

#[cfg(target_os = "macos")]
use super::super::{exec::run_timeout, overseer_home};
use crate::Result;

pub(crate) fn install_service() -> Result<()> {
    crate::setup::wizard::steps_service::install_service()
}

#[cfg(target_os = "macos")]
fn remove_legacy_service() -> Result<()> {
    let home = dirs::home_dir().ok_or(crate::Error::HomeDir)?;
    let legacy_plist = home.join("Library/LaunchAgents/com.robco.chief.plist");
    let mut id = Command::new("id");
    id.arg("-u");
    let output = run_timeout(id, Duration::from_secs(2))?;
    if !output.status.success() {
        return Err(crate::Error::Command {
            context: "look up user id for legacy launchd cleanup",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let uid = String::from_utf8_lossy(&output.stdout);
    let uid = uid.trim().parse::<u32>().map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid user id returned by `id -u`: {error}"),
        )
    })?;
    let mut command = Command::new("launchctl");
    command.args(["bootout", &format!("gui/{uid}/com.robco.chief")]);
    let output = run_timeout(command, Duration::from_secs(5))?;
    if !output.status.success() && !legacy_service_is_absent(output.status.code(), &output.stderr) {
        return Err(crate::Error::Command {
            context: "boot out legacy launchd service",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    if legacy_plist.try_exists()? {
        fs::remove_file(legacy_plist)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn legacy_service_is_absent(status: Option<i32>, stderr: &[u8]) -> bool {
    status == Some(3) && String::from_utf8_lossy(stderr).contains("No such process")
}

#[cfg(target_os = "macos")]
pub(crate) fn write_service_plist() -> Result<std::path::PathBuf> {
    remove_legacy_service()?;
    let home = dirs::home_dir().ok_or(crate::Error::HomeDir)?;
    let dir = home.join("Library/LaunchAgents");
    fs::create_dir_all(&dir)?;
    let path = dir.join("com.robco.overseer.plist");
    let executable = std::env::current_exe()?;
    let log = overseer_home()?.join("overseer.log");
    let path_env = service_path_env(std::env::var("PATH").ok().as_deref(), &home);
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.robco.overseer</string>
<key>ProgramArguments</key><array><string>{}</string><string>overseer</string><string>run</string></array>
<key>EnvironmentVariables</key><dict><key>PATH</key><string>{}</string></dict>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string>
</dict></plist>
"#,
        xml(&executable.to_string_lossy()),
        xml(&path_env),
        xml(&log.to_string_lossy()),
        xml(&log.to_string_lossy())
    );
    fs::write(&path, plist)?;
    Ok(path)
}

/// PATH for the launchd service. launchd agents get a bare system PATH
/// (`/usr/bin:/bin:/usr/sbin:/sbin`) that hides the tools the daemon shells
/// out to (dropr, tmux, git, the agent CLI). Start from the install-time
/// PATH and ensure the common tool dirs and system dirs are present.
#[cfg(target_os = "macos")]
fn service_path_env(current: Option<&str>, home: &std::path::Path) -> String {
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

#[cfg(target_os = "macos")]
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{legacy_service_is_absent, service_path_env};
    use std::path::Path;

    #[test]
    fn service_path_env_appends_missing_tool_dirs() {
        let path = service_path_env(
            Some("/usr/bin:/bin:/usr/sbin:/sbin"),
            Path::new("/Users/me"),
        );
        assert_eq!(
            path,
            "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin:/Users/me/.local/bin"
        );
    }

    #[test]
    fn service_path_env_keeps_existing_order() {
        let path = service_path_env(
            Some("/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:/Users/me/.local/bin"),
            Path::new("/Users/me"),
        );
        assert_eq!(
            path,
            "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:/Users/me/.local/bin:/usr/local/bin"
        );
    }

    #[test]
    fn service_path_env_builds_from_missing_path() {
        let path = service_path_env(None, Path::new("/Users/me"));
        assert_eq!(
            path,
            "/opt/homebrew/bin:/usr/local/bin:/Users/me/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }

    #[test]
    fn legacy_launchd_cleanup_only_tolerates_absent_service() {
        assert!(legacy_service_is_absent(
            Some(3),
            b"Boot-out failed: 3: No such process\n"
        ));
        assert!(!legacy_service_is_absent(
            Some(5),
            b"Boot-out failed: 5: Input/output error\n"
        ));
        assert!(!legacy_service_is_absent(
            Some(3),
            b"Boot-out failed: 3: Permission denied\n"
        ));
    }
}
