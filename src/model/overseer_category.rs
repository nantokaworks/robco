use std::path::PathBuf;

/// The rows of the OVERSEER frame, ordered by the question they answer
/// (dropr:357). `Inbox` and `Health` answer the two questions an operator
/// actually asks — is anything waiting on me, is anything stuck — and sit
/// first. `Ledger` and `Decisions` are the daemon's own bookkeeping, reachable
/// for debugging; dropr:378 folded them under a `Details` wrapper row that
/// carried nothing of its own, and dropr:469 retired that wrapper, so they sit
/// as top-level rows again. `Discord` (dropr:363) lists the retained
/// per-channel ops agents and sits last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverseerCategory {
    Inbox,
    Health,
    Ledger,
    Decisions,
    Discord,
}

impl OverseerCategory {
    /// Every row, in display order.
    pub const ALL: [Self; 5] = [
        Self::Inbox,
        Self::Health,
        Self::Ledger,
        Self::Decisions,
        Self::Discord,
    ];

    /// Sizes any per-category flag array indexed by [`Self::index`].
    pub const COUNT: usize = Self::ALL.len();

    /// Short and plain: an operator scanning the sidebar for what needs them
    /// reads the indicator on the row (dropr:469), not the label, so the label
    /// only has to name the row.
    ///
    /// English, always — category labels are UI structure, not content, so
    /// they stay English in every locale (dropr:377). The value doubles as a
    /// stable identifier: it is persisted verbatim in `ui_state.json`
    /// (`expanded_overseer_categories`) and used as an `item_key` for
    /// preview-tab memory, so localizing it would also silently reset that
    /// state on a language change.
    pub fn label(self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Health => "Health",
            Self::Ledger => "Ledger",
            Self::Decisions => "Decisions",
            Self::Discord => "Discord",
        }
    }

    /// Slot in any per-category flag array ([`Self::COUNT`] wide) — its
    /// position in [`Self::ALL`].
    pub fn index(self) -> usize {
        match self {
            Self::Inbox => 0,
            Self::Health => 1,
            Self::Ledger => 2,
            Self::Decisions => 3,
            Self::Discord => 4,
        }
    }

    /// Whether the category expands into rows of its own. Inbox and Discord
    /// do: Inbox's items are selection targets the operator answers, approves,
    /// or dismisses, and Discord's (dropr:371) are per-channel rows Enter can
    /// attach. Every other row expands into nothing: its detail is read-only
    /// text the Info preview already shows in full, so an arrow there would
    /// buy duplicated content at the cost of a nesting level the 24-column
    /// sidebar cannot afford.
    ///
    /// The single source of truth for the render, the input handling, and the
    /// persisted expansion state — none of them re-spell `matches!(_, Inbox)`.
    pub fn has_children(self) -> bool {
        matches!(self, Self::Inbox | Self::Discord)
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
    OverseerCategory(OverseerCategory),
    /// One aggregated Overseer Inbox item, indexing into [`crate::ui::App`]'s
    /// inbox list. Present only while the Inbox category is expanded, so the
    /// operator answers an escalation from the same cursor that walks the tree.
    OverseerInbox(usize),
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
