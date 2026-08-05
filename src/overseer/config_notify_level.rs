//! Discord notification verbosity: [`NotifyLevel`], the sole gate for which
//! events post, and the intrinsic [`NotifyTier`] an event belongs to. Split
//! out of `config.rs` to keep that file under the source size limit.

use serde::{Deserialize, Serialize};

/// Discord notification verbosity, ordered `off < errors < summary < all`.
/// Every Discord event has an intrinsic [`NotifyTier`]; a level admits an
/// event when the event's tier is at or below the level. This is the sole
/// gate for whether an event posts — see [`NotifyLevel::admits`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    /// Nothing fires.
    Off,
    /// Failures and blockers only: `task_failed`, `task_escalated`,
    /// `worker_blocked`, a circuit-open, and a generic escalation.
    Errors,
    /// Milestones + problems: everything in `Errors`, plus a successful
    /// task finish (`merged`) — "tell me when work lands or breaks, not
    /// every step". Merges landing close together roll up into one message
    /// (see `discord::rollup`); errors always fire immediately.
    #[default]
    Summary,
    /// Everything: `Summary` plus the step-by-step narration —
    /// `task_started`, `pr_opened`, and `queue_drained`.
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

    /// Whether this level admits an event at `tier`.
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
}
