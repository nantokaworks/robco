use std::path::PathBuf;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::{
    dropr::{DroprTaskFetch, DroprWorkspace},
    subagents::TaskSubagent,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoNode {
    pub path: PathBuf,
    pub name: String,
    pub remote_url: Option<String>,
    /// Persisted manual registration; keeps an agent-less repo listed.
    #[serde(default)]
    pub pinned: bool,
    /// Whether the Overseer treats this repo as its own to dispatch into and
    /// auto-merge for at all — the same vocabulary as `AgentNode::management`,
    /// reused at repo scope so the tree can show an agent row's marker only
    /// when it diverges from its repo's. Serde-defaulted to `Auto` so an
    /// existing `~/.robco/state.json` keeps today's behaviour until an
    /// operator opts a repo out.
    #[serde(default = "default_management_mode")]
    pub management: ManagementMode,
    #[serde(default)]
    pub agents: Vec<AgentNode>,
    #[serde(skip)]
    pub dropr: Option<DroprWorkspace>,
    /// Result of the last dropr task fetch, failures included: a pane that
    /// cannot tell a failed fetch from an empty board misreports both.
    #[serde(skip)]
    pub dropr_tasks: DroprTaskFetch,
    /// Status of the repo's own main-worktree AI session, or `None` when no such
    /// session is running (the main worktree does not auto-launch one). Runtime
    /// only; refreshed each tick and never persisted.
    #[serde(skip)]
    pub main_status: Option<Status>,
    #[serde(skip)]
    pub main_last_capture: Option<String>,
    /// Last observed spinner frame for main-session motion detection.
    #[serde(skip)]
    pub main_last_spinner: Option<String>,
    #[serde(skip)]
    pub main_last_change_at: Option<DateTime<Local>>,
    /// Whether the repo main-worktree companion shell (TERM) session is running
    /// a foreground command. Runtime only; refreshed each tick, never persisted.
    #[serde(skip)]
    pub main_shell_working: bool,
    /// Whether the main AI session has an in-flight tool call. Runtime only;
    /// refreshed each tick and never persisted.
    #[serde(skip)]
    pub main_mcp_active: bool,
    #[serde(skip)]
    pub main_pane_pid: Option<u32>,
    #[serde(skip)]
    pub main_tracked_command: Option<String>,
    #[serde(skip)]
    pub main_subagents_active: usize,
    /// How many commits the primary checkout's local `main` trails
    /// `origin/main` by, from whatever `origin/main` was last known to be —
    /// no network fetch of its own. `None` when it is not behind, or the
    /// comparison could not be made at all (no local `main`, no `origin`,
    /// not a repository). Runtime only; refreshed each tick, never
    /// persisted.
    #[serde(skip)]
    pub main_behind_origin: Option<u32>,
    /// Where the primary checkout's `HEAD` actually points, probed on the
    /// same slow discovery cadence as `main_behind_origin`, next to it.
    /// `None` when it is on `main` — the safe state — or the probe has not
    /// run yet; `Some` is always something a repo summary warns about.
    /// Runtime only; refreshed each tick, never persisted.
    #[serde(skip)]
    pub checkout_state: Option<CheckoutState>,
}

/// The one thing that can be wrong with the primary checkout's `HEAD`: it is
/// detached, or it is on a named branch that is not `main`. Both leave
/// `ready` (`overseer::release_pipeline`) unable to release and plain `git
/// pull` failing for the operator — see `RepoNode::checkout_state`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutState {
    Detached,
    OtherBranch(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNode {
    pub id: String,
    #[serde(default)]
    pub parent_agent_id: Option<String>,
    #[serde(default = "default_management_mode")]
    pub management: ManagementMode,
    pub title: String,
    /// The bare dropr task number (e.g. `"333"`) an Overseer-dispatched worker
    /// was created for, captured once at spawn time from the naming slug so
    /// the tree row can lead with it without re-deriving it from `branch` or
    /// `title` at render time. `None` for a manually-created or adopted agent,
    /// which carries no dropr task number at all.
    #[serde(default)]
    pub task_number: Option<String>,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub base_commit: String,
    pub program: String,
    #[serde(default)]
    pub claude_session_id: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    pub tmux_session: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    #[serde(skip)]
    pub status: Status,
    /// Whether the live AI session's worktree directory is missing. Runtime
    /// only; orthogonal to the captured AI status.
    #[serde(skip)]
    pub worktree_missing: bool,
    /// Detail from the latest failed native merge attempt. Runtime only.
    #[serde(skip)]
    pub merge_error: Option<String>,
    #[serde(skip)]
    pub last_capture: Option<String>,
    /// Last observed spinner frame for motion detection.
    #[serde(skip)]
    pub last_spinner: Option<String>,
    #[serde(skip)]
    pub last_change_at: Option<DateTime<Local>>,
    #[serde(skip)]
    pub last_auto_accept_at: Option<DateTime<Local>>,
    /// Whether the agent's companion shell (TERM) session is running a
    /// foreground command. Runtime only; refreshed each tick, never persisted.
    #[serde(skip)]
    pub shell_working: bool,
    /// Whether the AI session has an in-flight tool call. Runtime only;
    /// refreshed each tick and never persisted.
    #[serde(skip)]
    pub mcp_active: bool,
    #[serde(skip)]
    pub pane_pid: Option<u32>,
    #[serde(skip)]
    pub tracked_command: Option<String>,
    #[serde(skip)]
    pub subagents: Vec<TaskSubagent>,
    #[serde(skip)]
    pub children: Vec<ChildWorktree>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementMode {
    Auto,
    Manual,
}

fn default_management_mode() -> ManagementMode {
    ManagementMode::Auto
}

/// One agent's position in the identity tree's display order: its flat
/// index, its nesting depth, whether it is the last child among its
/// siblings, and — for each ancestor level above it, root first — whether
/// that ancestor still has a later sibling below. The tree connector needs
/// the last field to know, per row, whether to draw a continuing `│` guide
/// or leave that column blank while it descends through this row's subtree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    pub index: usize,
    pub depth: usize,
    pub is_last: bool,
    pub ancestor_continues: Vec<bool>,
}

/// Agent rows in display order, each carrying the sibling bookkeeping the
/// tree connector needs. `agent_order` is a depth-only view of the same
/// traversal for callers that don't render connectors.
pub fn agent_rows(agents: &[AgentNode]) -> Vec<AgentRow> {
    use std::collections::{HashMap, HashSet};

    let by_id: HashMap<&str, usize> = agents
        .iter()
        .enumerate()
        .map(|(index, agent)| (agent.id.as_str(), index))
        .collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); agents.len()];
    for (index, agent) in agents.iter().enumerate() {
        if let Some(parent) = agent
            .parent_agent_id
            .as_deref()
            .and_then(|id| by_id.get(id).copied())
        {
            children[parent].push(index);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn visit(
        index: usize,
        depth: usize,
        is_last: bool,
        ancestor_continues: &[bool],
        children: &[Vec<usize>],
        visited: &mut HashSet<usize>,
        ordered: &mut Vec<AgentRow>,
    ) {
        if !visited.insert(index) {
            return;
        }
        ordered.push(AgentRow {
            index,
            depth,
            is_last,
            ancestor_continues: ancestor_continues.to_vec(),
        });
        let kids = &children[index];
        let mut deeper = ancestor_continues.to_vec();
        deeper.push(!is_last);
        let last_child = kids.len().saturating_sub(1);
        for (position, &child) in kids.iter().enumerate() {
            visit(
                child,
                depth + 1,
                position == last_child,
                &deeper,
                children,
                visited,
                ordered,
            );
        }
    }

    let mut visited = HashSet::new();
    let mut ordered = Vec::with_capacity(agents.len());
    let roots: Vec<usize> = (0..agents.len())
        .filter(|&index| {
            !agents[index]
                .parent_agent_id
                .as_deref()
                .is_some_and(|id| by_id.contains_key(id))
        })
        .collect();
    let last_root = roots.len().saturating_sub(1);
    for (position, &index) in roots.iter().enumerate() {
        visit(
            index,
            0,
            position == last_root,
            &[],
            &children,
            &mut visited,
            &mut ordered,
        );
    }
    // Orphans left by a cycle where every member claims a parent: none of them
    // qualified as a root above, so nothing would visit them without this pass.
    for index in 0..agents.len() {
        if !visited.contains(&index) {
            visit(index, 0, true, &[], &children, &mut visited, &mut ordered);
        }
    }
    ordered
}

/// The single row for one agent index, for call sites that already know
/// which agent they're rendering rather than walking the full order.
pub fn agent_row(agents: &[AgentNode], index: usize) -> AgentRow {
    agent_rows(agents)
        .into_iter()
        .find(|row| row.index == index)
        .unwrap_or(AgentRow {
            index,
            depth: 0,
            is_last: true,
            ancestor_continues: Vec::new(),
        })
}

/// Agent indices and identity-tree depths in display order.
pub fn agent_order(agents: &[AgentNode]) -> Vec<(usize, usize)> {
    agent_rows(agents)
        .into_iter()
        .map(|row| (row.index, row.depth))
        .collect()
}

#[derive(Debug, Clone)]
pub struct ChildWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub clean: Option<bool>,
    pub ahead_behind: Option<(u32, u32)>,
    pub tmux_session: Option<String>,
    pub modified_at: Option<DateTime<Local>>,
}

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
            MergeLifecycle::ChecksRunning => "↻",
            MergeLifecycle::ChecksFailing => "‼",
            MergeLifecycle::OnHold => "⏸",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_labels_stay_english_in_every_locale() {
        use crate::locale::{Locale, t};
        // Top-level and nested alike: the Details children are still category
        // labels the sidebar renders, so they stay untranslated too.
        for category in OverseerCategory::ALL
            .into_iter()
            .chain(OverseerCategory::DETAILS_CHILDREN)
        {
            assert_eq!(t(Locale::En, category.label()), category.label());
            assert_eq!(t(Locale::Ja, category.label()), category.label());
        }
    }

    #[test]
    fn details_nests_ledger_and_decisions_out_of_the_top_level_rows() {
        for child in OverseerCategory::DETAILS_CHILDREN {
            assert!(!OverseerCategory::ALL.contains(&child), "{}", child.label());
            assert_eq!(child.parent(), Some(OverseerCategory::Details));
            assert!(!child.has_children(), "{}", child.label());
        }
        for category in OverseerCategory::ALL {
            assert_eq!(category.parent(), None, "{}", category.label());
        }
        // Every flag slot is distinct and in bounds, top-level and nested alike.
        let mut seen = std::collections::HashSet::new();
        for category in OverseerCategory::ALL
            .into_iter()
            .chain(OverseerCategory::DETAILS_CHILDREN)
        {
            assert!(category.index() < OverseerCategory::COUNT);
            assert!(seen.insert(category.index()), "{}", category.label());
        }
    }

    #[test]
    fn label_never_changes_with_locale_since_it_is_a_persisted_key() {
        for category in OverseerCategory::ALL {
            assert_eq!(category.label(), category.label());
        }
        // Regression guard for the persisted `ui_state.json` key and the
        // `item_key` preview-tab memory: `label()` takes no locale parameter
        // at all, so it cannot vary by config — this test exists to fail loudly
        // if a future edit adds one.
        let _: fn(OverseerCategory) -> &'static str = OverseerCategory::label;
    }

    #[test]
    fn status_badges_and_glyphs_are_stable() {
        assert_eq!(Status::Running.badge(), "run");
        assert_eq!(Status::Running.glyph(), "⠿");
        assert_eq!(Status::Waiting.glyph(), "?");
        assert_eq!(Status::Done.glyph(), "✓");
        assert_eq!(Status::Idle.glyph(), "·");
        assert_eq!(Status::Dead.glyph(), "✗");
        assert_eq!(Status::BranchOnly.glyph(), "⎇");
    }

    #[test]
    fn merge_lifecycle_glyphs_are_stable_and_distinct_from_status_glyphs() {
        let status_glyphs = [
            Status::Idle.glyph(),
            Status::Running.glyph(),
            Status::Waiting.glyph(),
            Status::Done.glyph(),
            Status::Dead.glyph(),
            Status::BranchOnly.glyph(),
        ];
        let lifecycle_glyphs = [
            MergeLifecycle::ChecksRunning.glyph(),
            MergeLifecycle::ChecksFailing.glyph(),
            MergeLifecycle::OnHold.glyph(),
        ];
        for glyph in lifecycle_glyphs {
            assert!(
                !status_glyphs.contains(&glyph),
                "{glyph} collides with a Status glyph"
            );
        }
        let mut seen = std::collections::HashSet::new();
        for glyph in lifecycle_glyphs {
            assert!(
                seen.insert(glyph),
                "{glyph} is not unique among MergeLifecycle glyphs"
            );
        }
    }

    #[test]
    fn agent_order_nests_children_and_keeps_cycles_visible() {
        fn agent(id: &str, parent: Option<&str>) -> AgentNode {
            let now = Local::now();
            AgentNode {
                id: id.into(),
                parent_agent_id: parent.map(str::to_string),
                management: ManagementMode::Manual,
                title: id.into(),
                task_number: None,
                worktree_path: PathBuf::from(id),
                branch: id.into(),
                base_commit: String::new(),
                program: String::new(),
                claude_session_id: None,
                profile: None,
                tmux_session: id.into(),
                created_at: now,
                updated_at: now,
                status: Status::Idle,
                worktree_missing: false,
                merge_error: None,
                last_capture: None,
                last_spinner: None,
                last_change_at: None,
                last_auto_accept_at: None,
                shell_working: false,
                mcp_active: false,
                pane_pid: None,
                tracked_command: None,
                subagents: Vec::new(),
                children: Vec::new(),
            }
        }
        let agents = vec![
            agent("parent", None),
            agent("other", None),
            agent("child", Some("parent")),
        ];
        assert_eq!(agent_order(&agents), vec![(0, 0), (2, 1), (1, 0)]);

        let cycle = vec![agent("a", Some("b")), agent("b", Some("a"))];
        assert_eq!(agent_order(&cycle), vec![(0, 0), (1, 1)]);

        let self_parent = vec![agent("self", Some("self")), agent("child", Some("self"))];
        assert_eq!(agent_order(&self_parent), vec![(0, 0), (1, 1)]);
    }

    #[test]
    fn agent_rows_mark_last_siblings_and_ancestor_guides() {
        fn agent(id: &str, parent: Option<&str>) -> AgentNode {
            let now = Local::now();
            AgentNode {
                id: id.into(),
                parent_agent_id: parent.map(str::to_string),
                management: ManagementMode::Manual,
                title: id.into(),
                task_number: None,
                worktree_path: PathBuf::from(id),
                branch: id.into(),
                base_commit: String::new(),
                program: String::new(),
                claude_session_id: None,
                profile: None,
                tmux_session: id.into(),
                created_at: now,
                updated_at: now,
                status: Status::Idle,
                worktree_missing: false,
                merge_error: None,
                last_capture: None,
                last_spinner: None,
                last_change_at: None,
                last_auto_accept_at: None,
                shell_working: false,
                mcp_active: false,
                pane_pid: None,
                tracked_command: None,
                subagents: Vec::new(),
                children: Vec::new(),
            }
        }

        // "parent" has a later root sibling "other", so its own row is not
        // last and carries no ancestors; "child" is parent's only child, so
        // it is last, but its ancestor ("parent") still has a later sibling,
        // so the guide over "child"'s row must continue.
        let agents = vec![
            agent("parent", None),
            agent("other", None),
            agent("child", Some("parent")),
        ];
        let rows = agent_rows(&agents);
        assert_eq!(
            rows,
            vec![
                AgentRow {
                    index: 0,
                    depth: 0,
                    is_last: false,
                    ancestor_continues: vec![],
                },
                AgentRow {
                    index: 2,
                    depth: 1,
                    is_last: true,
                    ancestor_continues: vec![true],
                },
                AgentRow {
                    index: 1,
                    depth: 0,
                    is_last: true,
                    ancestor_continues: vec![],
                },
            ]
        );

        // A deeper tree: "root" is the sole root (last), "mid-a" has a later
        // sibling "mid-b" (not last), and "leaf" hangs off "mid-a" alone
        // (last). "leaf"'s guide must be blank over "root"'s column (root has
        // no later sibling) but continue over "mid-a"'s column (mid-a does).
        let deeper = vec![
            agent("root", None),
            agent("mid-a", Some("root")),
            agent("mid-b", Some("root")),
            agent("leaf", Some("mid-a")),
        ];
        let rows = agent_rows(&deeper);
        let leaf = rows.iter().find(|row| row.index == 3).unwrap();
        assert_eq!(leaf.depth, 2);
        assert!(leaf.is_last);
        assert_eq!(leaf.ancestor_continues, vec![false, true]);
        let mid_a = rows.iter().find(|row| row.index == 1).unwrap();
        assert!(!mid_a.is_last);
        let mid_b = rows.iter().find(|row| row.index == 2).unwrap();
        assert!(mid_b.is_last);
    }

    #[test]
    fn merge_error_is_not_persisted() {
        let now = Local::now();
        let agent = AgentNode {
            id: "agent".into(),
            parent_agent_id: None,
            management: ManagementMode::Manual,
            title: "task".into(),
            task_number: None,
            worktree_path: "/tmp/task".into(),
            branch: "task".into(),
            base_commit: String::new(),
            program: "claude".into(),
            claude_session_id: None,
            profile: None,
            tmux_session: "robco_task".into(),
            created_at: now,
            updated_at: now,
            status: Status::Idle,
            worktree_missing: false,
            merge_error: Some("merge failed".into()),
            last_capture: None,
            last_spinner: None,
            last_change_at: None,
            last_auto_accept_at: None,
            shell_working: false,
            mcp_active: false,
            pane_pid: None,
            tracked_command: None,
            subagents: Vec::new(),
            children: Vec::new(),
        };

        let json = serde_json::to_string(&agent).unwrap();
        assert!(!json.contains("merge_error"));
        let restored: AgentNode = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.merge_error, None);
    }
}

/// The rows of the OVERSEER frame, ordered by the question they answer
/// (dropr:357). `Inbox` and `Health` answer the two questions an operator
/// actually asks — is anything waiting on me, is anything stuck — and sit
/// first. `Details` (dropr:378) folds the daemon's own bookkeeping away:
/// `Ledger` and `Decisions` are reachable for debugging, never deleted, but no
/// longer top-level rows — they nest under the collapsed `Details` row.
/// `Discord` (dropr:363) lists the retained per-channel ops agents and sits
/// last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverseerCategory {
    Inbox,
    Health,
    Details,
    Discord,
    /// Daemon bookkeeping, nested under [`Self::Details`] — never a top-level
    /// row (absent from [`Self::ALL`]), rendered only while `Details` is
    /// expanded.
    Ledger,
    /// See [`Self::Ledger`].
    Decisions,
}

impl OverseerCategory {
    /// The top-level rows, in display order. `Ledger` and `Decisions` are not
    /// here on purpose: they render as children of `Details`.
    pub const ALL: [Self; 4] = [Self::Inbox, Self::Health, Self::Details, Self::Discord];

    /// The rows the expanded `Details` category nests, in display order.
    pub const DETAILS_CHILDREN: [Self; 2] = [Self::Ledger, Self::Decisions];

    /// Every variant, top-level or nested — sizes any per-category flag array
    /// indexed by [`Self::index`].
    pub const COUNT: usize = 6;

    /// "Waiting on you" rather than "Inbox": the row answers a question, and an
    /// operator scanning the sidebar for what needs them should not have to
    /// know dropr/robco jargon for a mail metaphor to find it.
    ///
    /// English, always — category labels are UI structure, not content, so
    /// they stay English in every locale (dropr:377). The value doubles as a
    /// stable identifier: it is persisted verbatim in `ui_state.json`
    /// (`expanded_overseer_categories`) and used as an `item_key` for
    /// preview-tab memory, so localizing it would also silently reset that
    /// state on a language change.
    pub fn label(self) -> &'static str {
        match self {
            Self::Inbox => "Waiting on you",
            Self::Health => "Health",
            Self::Details => "Details",
            Self::Ledger => "Ledger",
            Self::Decisions => "Decisions",
            Self::Discord => "Discord",
        }
    }

    /// Slot in any per-category flag array ([`Self::COUNT`] wide). The first
    /// [`Self::ALL`]`.len()` slots are the top-level rows in display order; the
    /// `Details` children take the trailing slots, which stay `false` forever —
    /// a child row has no expansion of its own.
    pub fn index(self) -> usize {
        match self {
            Self::Inbox => 0,
            Self::Health => 1,
            Self::Details => 2,
            Self::Discord => 3,
            Self::Ledger => 4,
            Self::Decisions => 5,
        }
    }

    /// The category a nested row folds away under, or `None` for a top-level
    /// row. Collapsing a child row collapses this parent, the same way an
    /// inbox item folds away under `Inbox`.
    pub fn parent(self) -> Option<Self> {
        match self {
            Self::Ledger | Self::Decisions => Some(Self::Details),
            _ => None,
        }
    }

    /// Whether the category expands into rows of its own. Inbox and Discord
    /// do: Inbox's items are selection targets the operator answers, approves,
    /// or dismisses, and Discord's (dropr:371) are per-channel rows Enter can
    /// attach. Details (dropr:378) expands into the Ledger and Decisions child
    /// rows it folded away. Health expands into nothing: its detail is
    /// read-only text the Info preview already shows in full, so an arrow
    /// there would buy duplicated content at the cost of a nesting level the
    /// 24-column sidebar cannot afford.
    ///
    /// The single source of truth for the render, the input handling, and the
    /// persisted expansion state — none of them re-spell `matches!(_, Inbox)`.
    pub fn has_children(self) -> bool {
        matches!(self, Self::Inbox | Self::Discord | Self::Details)
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
