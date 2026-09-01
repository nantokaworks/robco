#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Idle,
    Running,
    Waiting,
    /// The AI finished a turn and is sitting at its input prompt with nothing
    /// pending — distinct from `Waiting` (a real y/n / selection prompt) and
    /// from `Idle` (a session that has done nothing yet).
    Done,
    Dead,
    BranchOnly,
}

impl Status {
    pub fn badge(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Running => "run",
            Status::Waiting => "wait",
            Status::Done => "done",
            Status::Dead => "dead",
            Status::BranchOnly => "branch",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Status::Idle => "·",
            Status::Running => "⠿",
            Status::Waiting => "?",
            Status::Done => "✓",
            Status::Dead => "✗",
            Status::BranchOnly => "⎇",
        }
    }
}

/// Where a worker's pull request actually stands once the AI session itself
/// has gone quiet (`Status::Done`) — the session finishing a turn says
/// nothing about whether the branch merged, so this is read from the
/// Overseer ledger entry instead. See
/// `crate::ui::overseer::OverseerSnapshot::merge_lifecycle`.
///
/// Deliberately excludes an actually-merged pull request: that case is
/// genuinely finished, so it keeps rendering as the plain `Status::Done`
/// glyph rather than a lifecycle glyph of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeLifecycle {
    /// An operator approval is scoped to the current head and waiting for the
    /// deterministic merge gate.
    ApprovedWaiting,
    /// The auto-merge gate is waiting on CI checks to finish.
    ChecksRunning,
    /// CI checks came back red.
    ChecksFailing,
    /// Held for a reason outside the two above (behind its base branch,
    /// missing branch protection, and the like). The detail lives in the
    /// Info pane rather than in the glyph vocabulary.
    OnHold,
}

impl MergeLifecycle {
    pub fn glyph(self) -> &'static str {
        match self {
            MergeLifecycle::ApprovedWaiting => "◆",
            MergeLifecycle::ChecksRunning => "↻",
            MergeLifecycle::ChecksFailing => "‼",
            MergeLifecycle::OnHold => "⏸",
        }
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
