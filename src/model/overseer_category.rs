use std::path::PathBuf;

/// Expandable sections in the OVERSEER frame.
///
/// Operational state is shown where it belongs: warnings under the header and
/// escalations under their worker or repository. Discord remains as the only
/// category because it owns selectable child rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverseerCategory {
    Discord,
}

impl OverseerCategory {
    /// Every row, in display order.
    pub const ALL: [Self; 1] = [Self::Discord];

    /// Sizes any per-category flag array indexed by [`Self::index`].
    pub const COUNT: usize = Self::ALL.len();

    /// English, always — category labels are UI structure, not content, so
    /// they stay English in every locale (dropr:377). The value doubles as a
    /// stable identifier: it is persisted verbatim in `ui_state.json`
    /// (`expanded_overseer_categories`) and used as an `item_key` for
    /// preview-tab memory, so localizing it would also silently reset that
    /// state on a language change.
    pub fn label(self) -> &'static str {
        match self {
            Self::Discord => "Discord",
        }
    }

    /// Slot in any per-category flag array ([`Self::COUNT`] wide) — its
    /// position in [`Self::ALL`].
    pub fn index(self) -> usize {
        match self {
            Self::Discord => 0,
        }
    }

    /// Whether the category expands into rows of its own. Discord's retained
    /// channels are selectable rows that Enter can attach.
    ///
    /// The single source of truth for the render, the input handling, and the
    /// persisted expansion state.
    pub fn has_children(self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// The Overseer's own control AI: a row of its own rather than a preview
    /// tab, so Enter can attach it (creating the tmux session when absent) the
    /// same way Enter attaches any other AI session. Sits above the categories
    /// — it is the one OVERSEER row that owns a session to attach to, not a
    /// read-only summary of one (dropr:370).
    OverseerAi,
    /// A repo-less escalation shown directly below OVERSEER warnings.
    OverseerAlert(usize),
    OverseerCategory(OverseerCategory),
    /// One retained per-channel Discord ops agent, indexing into the same
    /// newest-active-first channel order the Discord category's detail rows
    /// render (dropr:371). Present only while the Discord category is
    /// expanded, so Enter can attach the channel's live tmux session — a
    /// session that exists only while a turn is running, torn down at the
    /// end of each turn.
    DiscordChannel(usize),
    /// A connected remote host's Overseer control AI, indexing `App::hosts`.
    RemoteControlAi(usize),
    /// A retained remote Discord channel in that host snapshot's display order.
    RemoteDiscordChannel {
        host: usize,
        channel: usize,
    },
    Repo(usize),
    /// An escalation whose repository remains but whose worker row is gone.
    RepoEscalation {
        repo: usize,
        item: usize,
    },
    Agent {
        repo: usize,
        agent: usize,
    },
    ChildWorktree {
        repo: usize,
        agent: usize,
        child: usize,
    },
    /// Collapsible header of the "other locations" section listing repos that
    /// live outside the launch directory but still have agents.
    OtherHeader,
    /// Collapsible header of the "orphan sessions" section listing
    /// robco-prefixed tmux sessions no tracked agent or repo accounts for.
    OrphanHeader,
    /// One orphan session row, indexing into [`crate::ui::App`]'s orphan list.
    Orphan(usize),
}

/// A live robco-prefixed tmux session that neither a tracked agent (or its
/// `-shell` twin) nor a registry repo's derived main session accounts for —
/// e.g. left behind by a pre-#66 registry wipe or a deleted worktree. Runtime
/// only; rebuilt from `tmux` on each discovery tick and never persisted.
#[derive(Debug, Clone)]
pub struct OrphanSession {
    pub name: String,
    pub cwd: PathBuf,
}

#[cfg(test)]
#[path = "overseer_category_tests.rs"]
mod tests;
