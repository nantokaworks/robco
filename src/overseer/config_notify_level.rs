//! Discord notification verbosity: [`NotifyLevel`], the intrinsic
//! [`NotifyTier`] an event belongs to, and [`notify_admits`], which
//! resolves the two against a legacy per-event boolean override. Split out
//! of `config.rs` to keep that file under the source size limit.

use serde::{Deserialize, Serialize};

/// Discord notification verbosity, ordered `off < errors < summary < all`.
/// Every Discord event has an intrinsic [`NotifyTier`]; a level admits an
/// event when the event's tier is at or below the level. This is the
/// *baseline* only — see [`notify_admits`] for how it composes with the
/// seven legacy per-event booleans on `DiscordConfig`, which still take
/// precedence when explicitly set.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    /// Nothing fires.
    Off,
    /// Failures and blockers only: `task_failed`, `task_escalated`,
    /// `worker_blocked`, a circuit-open, and a generic escalation.
    Errors,
    /// Errors, plus `task_started`, a successful task finish (`merged`),
    /// and `queue_drained` — "tell me it started, tell me it finished, tell
    /// me when something breaks."
    #[default]
    Summary,
    /// Everything, including `pr_opened` — today's unconditional behavior.
    All,
}

impl NotifyLevel {
    fn rank(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Errors => 1,
            Self::Summary => 2,
            Self::All => 3,
        }
    }

    /// Whether this level admits an event at `tier` on its own, before any
    /// per-event boolean override in [`notify_admits`] is applied.
    pub fn admits(self, tier: NotifyTier) -> bool {
        self.rank() >= tier.rank()
    }
}

/// The intrinsic verbosity tier one Discord event belongs to, compared
/// against a [`NotifyLevel`]'s rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyTier {
    Errors,
    Summary,
    All,
}

impl NotifyTier {
    fn rank(self) -> u8 {
        match self {
            Self::Errors => 1,
            Self::Summary => 2,
            Self::All => 3,
        }
    }
}

/// Resolves whether one event fires, composing `notify_level` with a
/// per-event legacy boolean.
///
/// **Precedence: an explicitly-set boolean always wins; the level is only
/// the baseline for an event no boolean addresses.** Every Discord config
/// ever saved by this program serializes its seven per-event booleans
/// explicitly (there is no `skip_serializing_if` that would have omitted
/// them), so this keeps every config file written before `notify_level`
/// existed behaving exactly as before — the level has no effect on an
/// installation that already says what it wants. `notify_level` only takes
/// over once an operator (or the install wizard) actually unsets a
/// boolean — which is what a *fresh* `DiscordConfig::default()` does for
/// all seven, so a new install is governed by the level from the start.
/// The alternative precedence directions were rejected: "both must allow"
/// would silently mute an event an existing config explicitly turned on
/// the moment an operator lowered the level, and "level wins outright"
/// would break every existing config's explicit `true` the day this
/// shipped.
pub fn notify_admits(explicit: Option<bool>, level: NotifyLevel, tier: NotifyTier) -> bool {
    explicit.unwrap_or_else(|| level.admits(tier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_admits_its_own_tier_and_everything_quieter() {
        assert!(NotifyLevel::Errors.admits(NotifyTier::Errors));
        assert!(!NotifyLevel::Errors.admits(NotifyTier::Summary));
        assert!(!NotifyLevel::Errors.admits(NotifyTier::All));

        assert!(NotifyLevel::Summary.admits(NotifyTier::Errors));
        assert!(NotifyLevel::Summary.admits(NotifyTier::Summary));
        assert!(!NotifyLevel::Summary.admits(NotifyTier::All));

        assert!(NotifyLevel::All.admits(NotifyTier::Errors));
        assert!(NotifyLevel::All.admits(NotifyTier::Summary));
        assert!(NotifyLevel::All.admits(NotifyTier::All));
    }

    #[test]
    fn off_admits_nothing() {
        for tier in [NotifyTier::Errors, NotifyTier::Summary, NotifyTier::All] {
            assert!(!NotifyLevel::Off.admits(tier));
        }
    }

    #[test]
    fn explicit_boolean_overrides_the_level_in_either_direction() {
        assert!(notify_admits(Some(true), NotifyLevel::Off, NotifyTier::All));
        assert!(!notify_admits(
            Some(false),
            NotifyLevel::All,
            NotifyTier::Errors
        ));
    }

    #[test]
    fn unset_boolean_defers_to_the_level() {
        assert!(notify_admits(
            None,
            NotifyLevel::Summary,
            NotifyTier::Summary
        ));
        assert!(!notify_admits(None, NotifyLevel::Summary, NotifyTier::All));
    }
}
