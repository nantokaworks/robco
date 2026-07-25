//! The one merge strategy every merge path reads.
//!
//! `gh pr merge` used to be handed its flag from two independent keys: the TUI's
//! `m` key read the top-level `merge_strategy`, the Overseer's auto-merge gate
//! read `overseer.merge_strategy`, and the two defaulted separately. The
//! divergence only shows when the flags disagree *and* the branch cannot take
//! one of them — a head branch carrying a merge commit is merged by `--squash`
//! and `--merge` and refused by `--rebase` — so the same pull request would
//! merge unattended and fail under the operator's hand.
//!
//! There is one key now. The nested one is still read, but only so a config
//! that carries it can be migrated: [`resolve`] holds the rule, and the key is
//! never written back.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MergeStrategy {
    Rebase,
    /// The default. It is what the unattended path has been merging on, and the
    /// only strategy no branch shape can refuse — a default that can be refused
    /// fails where nobody is watching.
    #[default]
    Squash,
    Merge,
}

impl MergeStrategy {
    pub fn gh_flag(self) -> &'static str {
        match self {
            MergeStrategy::Rebase => "--rebase",
            MergeStrategy::Squash => "--squash",
            MergeStrategy::Merge => "--merge",
        }
    }

    /// The name the config file, the decision log, and the merge dialog all use.
    pub fn label(self) -> &'static str {
        match self {
            MergeStrategy::Rebase => "rebase",
            MergeStrategy::Squash => "squash",
            MergeStrategy::Merge => "merge",
        }
    }

    /// Reads a legacy `overseer.merge_strategy` string. An unrecognised value
    /// maps to `Squash`, exactly as the daemon's own match arm did, so a config
    /// with a typo in it keeps the behaviour it already had rather than failing
    /// to load.
    fn from_legacy(raw: &str) -> Self {
        match raw.trim() {
            "merge" => MergeStrategy::Merge,
            "rebase" => MergeStrategy::Rebase,
            _ => MergeStrategy::Squash,
        }
    }
}

/// The strategy both merge paths will use, and what the operator should be told
/// about how it was reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMergeStrategy {
    pub strategy: MergeStrategy,
    /// Set only when the legacy key decided the answer, because that is the
    /// case where the file no longer says what robco is about to do. Surfaced
    /// on the TUI banner and in the daemon log at startup.
    pub notice: Option<String>,
}

/// Reconciles the top-level `merge_strategy` with a legacy `overseer.merge_strategy`.
///
/// Both arguments are `None` when the key is absent from the file, which is what
/// separates a value written explicitly from one that merely took the default.
///
/// - Neither written: the default, silently.
/// - Top-level only: that value, silently — it is already the single key.
/// - Legacy only: that value, reported. The key it came from is about to
///   disappear from the file, so the file would otherwise stop explaining the
///   behaviour it produces.
/// - Both, agreeing: that value, silently. Dropping the redundant key changes
///   nothing anyone can observe.
/// - Both, disagreeing: the Overseer's value wins, reported. It is the value the
///   unattended merges have actually been landing on, so keeping it leaves the
///   path nobody is watching working exactly as it was and moves the
///   interactive path — the one an operator can see fail and retry — onto it.
pub fn resolve(top_level: Option<MergeStrategy>, legacy: Option<&str>) -> ResolvedMergeStrategy {
    let Some(legacy) = legacy.map(MergeStrategy::from_legacy) else {
        return ResolvedMergeStrategy {
            strategy: top_level.unwrap_or_default(),
            notice: None,
        };
    };
    match top_level {
        Some(top_level) if top_level == legacy => ResolvedMergeStrategy {
            strategy: legacy,
            notice: None,
        },
        Some(top_level) => ResolvedMergeStrategy {
            strategy: legacy,
            notice: Some(format!(
                "merge strategy: merge_strategy \"{}\" and overseer.merge_strategy \"{}\" \
                 disagreed; both paths now use \"{}\", the one the Overseer was already \
                 merging on",
                top_level.label(),
                legacy.label(),
                legacy.label(),
            )),
        },
        None => ResolvedMergeStrategy {
            strategy: legacy,
            notice: Some(format!(
                "merge strategy: adopted overseer.merge_strategy \"{}\" as the single \
                 merge_strategy",
                legacy.label(),
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategies_map_to_gh_flags_and_labels() {
        assert_eq!(MergeStrategy::default(), MergeStrategy::Squash);
        assert_eq!(MergeStrategy::Rebase.gh_flag(), "--rebase");
        assert_eq!(MergeStrategy::Squash.gh_flag(), "--squash");
        assert_eq!(MergeStrategy::Merge.gh_flag(), "--merge");
        assert_eq!(MergeStrategy::Rebase.label(), "rebase");
        assert_eq!(MergeStrategy::Squash.label(), "squash");
        assert_eq!(MergeStrategy::Merge.label(), "merge");
    }

    #[test]
    fn neither_key_takes_the_default_without_comment() {
        assert_eq!(
            resolve(None, None),
            ResolvedMergeStrategy {
                strategy: MergeStrategy::Squash,
                notice: None,
            }
        );
    }

    #[test]
    fn the_top_level_key_alone_is_the_answer() {
        assert_eq!(
            resolve(Some(MergeStrategy::Rebase), None),
            ResolvedMergeStrategy {
                strategy: MergeStrategy::Rebase,
                notice: None,
            }
        );
    }

    /// The migration case: the key that decided this is about to leave the file,
    /// so the operator is told which value survived it.
    #[test]
    fn the_legacy_key_alone_is_adopted_and_reported() {
        let resolved = resolve(None, Some("merge"));
        assert_eq!(resolved.strategy, MergeStrategy::Merge);
        let notice = resolved.notice.expect("adoption is reported");
        assert!(notice.contains("overseer.merge_strategy"), "{notice}");
        assert!(notice.contains("\"merge\""), "{notice}");
    }

    #[test]
    fn conflicting_keys_keep_the_overseer_value_and_report_the_conflict() {
        let resolved = resolve(Some(MergeStrategy::Rebase), Some("squash"));
        assert_eq!(resolved.strategy, MergeStrategy::Squash);
        let notice = resolved.notice.expect("a conflict is reported");
        assert!(notice.contains("\"rebase\""), "{notice}");
        assert!(notice.contains("\"squash\""), "{notice}");
    }

    /// Agreeing keys are not a conflict: dropping the redundant one changes
    /// nothing, so it is not worth a banner.
    #[test]
    fn agreeing_keys_are_silent() {
        assert_eq!(
            resolve(Some(MergeStrategy::Rebase), Some("rebase")),
            ResolvedMergeStrategy {
                strategy: MergeStrategy::Rebase,
                notice: None,
            }
        );
    }

    /// A value the daemon never understood merged as a squash; it still does,
    /// rather than refusing to load the config it has always accepted.
    #[test]
    fn an_unrecognised_legacy_value_still_squashes() {
        assert_eq!(resolve(None, Some("sqush")).strategy, MergeStrategy::Squash);
    }
}
