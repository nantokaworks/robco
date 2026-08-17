use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// How strictly the auto-merge gate requires the base branch to be protected.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "kebab-case")]
pub enum ProtectionMode {
    /// Require both a pull-request requirement and at least one required status check.
    #[default]
    Required,
    /// Require only that changes go through a pull request.
    Relaxed,
    /// Skip the protection probe and rely on GitHub's mergeability signal alone.
    Off,
}

impl ProtectionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Relaxed => "relaxed",
            Self::Off => "off",
        }
    }
}
