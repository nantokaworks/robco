//! The credential channel a daemon-spawned session resolves its environment from.
//!
//! A launchd agent is not a login session: it gets the `EnvironmentVariables`
//! its plist declares and nothing else, and the macOS keychain item the Claude
//! CLI refreshes belongs to the user's interactive session rather than to the
//! agent. A session spawned as a direct child of the daemon therefore cannot
//! borrow the credential an interactive `claude` run uses, which is why every
//! judge session started under the service died on authentication while
//! dispatched workers — children of the login-owned tmux server — kept working.
//!
//! Service daemons answer this with an explicit, non-interactive channel rather
//! than by reaching into a login session's secret store: systemd has
//! `Environment=` / `EnvironmentFile=`, the AWS CLI resolves `AWS_*` before
//! `~/.aws/credentials`, `gh` reads `GH_TOKEN` before `hosts.yml`. Robco's
//! channel is the same shape, and resolves in this order — first hit wins per
//! variable name:
//!
//! 1. `overseer.session_env` in `~/.robco/config.json`.
//! 2. The env file, `overseer.session_env_file` or `~/.robco/env` by default.
//! 3. Whatever the daemon process itself inherited.
//!
//! Only the first two produce explicit assignments; the third is plain
//! inheritance, so a name nobody configured keeps whatever the daemon has.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::config::{Config, expand_tilde, robco_dir};

/// Environment names robco recognises as carrying a session credential. The
/// list decides nothing — it only lets the health surface name which variable
/// is standing in for the keychain, so an operator can tell a configured
/// channel from an empty one.
pub(crate) const CREDENTIAL_NAMES: [&str; 3] = [
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
];

/// Env file consulted when `overseer.session_env_file` is unset.
pub(crate) const DEFAULT_ENV_FILE: &str = "env";

/// Which layer of the resolution order an assignment came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnvSource {
    Config,
    File,
    Process,
}

impl EnvSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Config => "overseer.session_env",
            Self::File => "session env file",
            Self::Process => "daemon process environment",
        }
    }
}

/// A credential the channel supplies, named for the health surface. The value
/// is deliberately absent: nothing outside the spawned process needs it, and a
/// secret that never leaves this module cannot be logged by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Credential {
    pub(crate) name: String,
    pub(crate) source: EnvSource,
}

/// The explicit assignments applied to every daemon-spawned session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionEnv {
    vars: BTreeMap<String, (String, EnvSource)>,
}

impl SessionEnv {
    /// Resolve the channel for `config`, reading the env file if one exists.
    pub(crate) fn resolve(config: &Config) -> Self {
        Self::resolve_at(config, env_file_path(config).as_deref())
    }

    /// Build a channel directly, for tests that must not depend on whether the
    /// machine running them happens to have a `~/.robco/env`.
    #[cfg(test)]
    pub(crate) fn from_config_vars(vars: &[(&str, &str)]) -> Self {
        Self {
            vars: vars
                .iter()
                .map(|(name, value)| {
                    (
                        (*name).to_string(),
                        ((*value).to_string(), EnvSource::Config),
                    )
                })
                .collect(),
        }
    }

    fn resolve_at(config: &Config, file: Option<&Path>) -> Self {
        let mut vars = BTreeMap::new();
        if let Some(raw) = file.and_then(|path| fs::read_to_string(path).ok()) {
            for (name, value) in parse_env_file(&raw) {
                vars.insert(name, (value, EnvSource::File));
            }
        }
        for (name, value) in &config.overseer.session_env {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            vars.insert(name.to_string(), (value.clone(), EnvSource::Config));
        }
        Self { vars }
    }

    /// Set every explicit assignment on `command`, so the spawned session sees
    /// the configured value rather than whatever the daemon inherited.
    pub(crate) fn apply(&self, command: &mut Command) {
        for (name, (value, _)) in &self.vars {
            command.env(name, value);
        }
    }

    /// The assignments as tmux-launcher environment pairs.
    pub(crate) fn pairs(&self) -> Vec<(String, String)> {
        self.vars
            .iter()
            .map(|(name, (value, _))| (name.clone(), value.clone()))
            .collect()
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.vars.keys().map(String::as_str)
    }

    /// The credential this channel supplies, if any. Configured assignments are
    /// checked before the daemon's own environment for the same reason the
    /// resolution order puts them there: a value the operator wrote down is the
    /// one they expect the session to use.
    pub(crate) fn credential(&self) -> Option<Credential> {
        self.credential_with(|name| std::env::var(name).ok())
    }

    fn credential_with(&self, inherited: impl Fn(&str) -> Option<String>) -> Option<Credential> {
        CREDENTIAL_NAMES.iter().find_map(|name| {
            self.vars
                .get(*name)
                .filter(|(value, _)| !value.trim().is_empty())
                .map(|(_, source)| Credential {
                    name: (*name).to_string(),
                    source: *source,
                })
                .or_else(|| {
                    inherited(name)
                        .filter(|value| !value.trim().is_empty())
                        .map(|_| Credential {
                            name: (*name).to_string(),
                            source: EnvSource::Process,
                        })
                })
        })
    }
}

/// Where the env file lives: the configured path with `~` expanded, or
/// `~/.robco/env`. A configured path that cannot be resolved yields `None`
/// rather than falling back, so a typo is a missing file and not a silent read
/// of a different one.
fn env_file_path(config: &Config) -> Option<PathBuf> {
    match &config.overseer.session_env_file {
        Some(path) => Some(expand_tilde(path)),
        None => robco_dir().ok().map(|dir| dir.join(DEFAULT_ENV_FILE)),
    }
}

/// Parse `KEY=VALUE` lines the way every `EnvironmentFile=` consumer does:
/// blank lines and `#` comments are skipped, a leading `export ` is tolerated
/// so a file can be `source`d by a shell too, and one layer of matching quotes
/// is stripped from the value.
fn parse_env_file(raw: &str) -> Vec<(String, String)> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (name, value) = line.split_once('=')?;
            let name = name.trim();
            (!name.is_empty() && name.chars().all(is_env_name_char))
                .then(|| (name.to_string(), unquote(value.trim()).to_string()))
        })
        .collect()
}

fn is_env_name_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;
