//! Command groups that used to live under the retired `overseer` namespace.
//! Each group here keeps a level because the level groups a noun (`config`,
//! `inbox`, `service`, `decisions`); the daemon-lifecycle verbs (`status`,
//! `start`, `stop`, `restart`, `daemon`, `panic`) moved straight to the top
//! level instead, in `cli::Command`.

use clap::{Args as ClapArgs, Subcommand, ValueEnum};

use crate::overseer::config::ProtectionMode;

#[derive(Debug, ClapArgs)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Persist a runtime toggle in RobCo's JSON config.
    Set(OverseerSetArgs),
    /// Choose the channel that receives reports (decision notifications and
    /// digests). Cleared, reports fall back to the chat channel.
    NotifyChannel(OverseerNotifyChannelArgs),
    /// Set how strictly the merge gate requires the base branch to be protected.
    Protection(OverseerProtectionArgs),
}

#[derive(Debug, ClapArgs)]
pub struct InboxArgs {
    #[command(subcommand)]
    pub command: InboxCommand,
}

#[derive(Debug, Subcommand)]
pub enum InboxCommand {
    /// Hide every currently listed Inbox item. Suppression only: the decision
    /// log and the ledger are left alone, and a newer escalation for the same
    /// target is listed again.
    Clear,
}

#[derive(Debug, ClapArgs)]
pub struct ServiceArgs {
    #[command(subcommand)]
    pub command: ServiceCommand,
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    /// Write a launchd service plist and load or reload it after confirmation.
    Install,
}

#[derive(Debug, ClapArgs)]
pub struct DecisionsArgs {
    #[command(subcommand)]
    pub command: DecisionsCommand,
}

#[derive(Debug, Subcommand)]
pub enum DecisionsCommand {
    /// Quarantine unparseable decision-log lines to a sidecar file, keeping
    /// every valid line byte-identical and in order. Safe to run while the
    /// daemon is appending.
    Compact(OverseerCompactDecisionsArgs),
}

#[derive(Debug, ClapArgs)]
pub struct OverseerSetArgs {
    #[arg(value_enum)]
    pub setting: OverseerSetting,
    #[arg(value_enum)]
    pub value: OnOff,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerStatusArgs {
    /// Also print internal bookkeeping: the raw ledger phase tally, `workers by
    /// repo` keyed by absolute path, the skip list, and the recent decision-log
    /// tail. Nothing here is deleted from the daemon's records — this only
    /// changes what the command prints by default.
    #[arg(long)]
    pub debug: bool,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerNotifyChannelArgs {
    /// Discord channel id that receives reports.
    #[arg(required_unless_present = "clear")]
    pub channel_id: Option<String>,
    /// Clear the report channel; reports fall back to the chat channel.
    #[arg(long, conflicts_with = "channel_id")]
    pub clear: bool,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerProtectionArgs {
    /// `required` demands a pull-request rule and required status checks, `relaxed`
    /// demands only the pull-request rule, `off` skips the probe entirely.
    #[arg(value_enum)]
    pub mode: ProtectionMode,
}

#[derive(Debug, ClapArgs)]
pub struct OverseerCompactDecisionsArgs {
    /// Report kept/quarantined counts without rewriting the log.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OverseerSetting {
    AutoMerge,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OnOff {
    On,
    Off,
}

impl OnOff {
    pub fn enabled(self) -> bool {
        matches!(self, Self::On)
    }
}
