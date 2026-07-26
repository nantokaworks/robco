use std::{
    collections::{HashMap, HashSet},
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use crate::{
    Result,
    config::Config,
    model::{ManagementMode, OverseerCategory, Selection},
    registry::Registry,
};

use actions::{
    background_refresh::BackgroundRefresh, dropr_tasks::DroprTaskRefresh,
    preview_capture::PreviewCapture,
};

/// How often the launch directory and each repo's worktrees are re-scanned to
/// pick up projects or worktrees created outside robco.
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(3);

mod actions;
mod blockfont;
mod confirm_pr;
#[cfg(test)]
mod confirm_pr_tests;
mod dialog;
mod error_dialog;
mod event_loop;
mod help;
pub(crate) mod inbox;
mod input;
mod input_wrap;
mod layout;
mod list;
mod merge_dialog;
mod overseer;
mod preview;
mod registry_write;
mod repo_description;
mod scrollback;
#[cfg(test)]
mod sidebar_frame_tests;
pub(crate) mod spinner;
mod summary;
#[cfg(test)]
mod tests;
mod theme;
mod tree;

pub use event_loop::run;

enum Mode {
    Normal,
    Help {
        scroll: u16,
    },
    PromptAgent {
        repo: usize,
        input: String,
    },
    PromptRepo {
        input: String,
    },
    PromptOverseer {
        input: String,
    },
    PromptInbox {
        target_session: String,
        label: String,
        input: String,
    },
    ConfirmKill {
        repo: usize,
        agent: usize,
    },
    /// Move every worker under one repo the overseer may touch to `target` at
    /// once, enrolling unmanaged worktrees when `target` is Auto. `count` is
    /// how many of them the dialog expects to change; the applied count is
    /// recomputed under the registry lock.
    ConfirmOverseerBulkToggle {
        repo_path: PathBuf,
        repo_name: String,
        target: ManagementMode,
        count: usize,
    },
    ConfirmRemoveRepo {
        path: PathBuf,
    },
    ConfirmMerge {
        repo: usize,
        agent: usize,
    },
    /// The agent's pull request already merged, so `m` offers the cleanup that
    /// normally follows a merge instead of a merge that has nothing left to do.
    ConfirmCleanup {
        repo: usize,
        agent: usize,
    },
    ConfirmPr {
        repo_path: PathBuf,
        agent_id: String,
        branch: String,
        input: String,
    },
    ConfirmDeleteBranch {
        repo: usize,
        agent: usize,
    },
    ErrorDialog {
        title: String,
        lines: Vec<String>,
        force_kill: Option<ForceKillTarget>,
    },
    // Holds the session NAME, not an index into `App::orphans` — the orphan
    // list is rebuilt on every discovery tick, so an index captured when the
    // dialog opened could point at a different session by the time the user
    // confirms. The name pins the kill to exactly what the dialog displayed.
    ConfirmKillOrphan {
        session: String,
    },
    // Panic-stop the overseer: disable dispatch and kill every overseer-managed
    // worker. Reachable only while an OVERSEER row is selected.
    ConfirmOverseerPanic,
    /// Reset the overseer dispatch circuit: re-enable dispatch and clear the
    /// failure counter. Reachable only while the overseer panel is visible and
    /// the circuit is open.
    ConfirmOverseerReset,
    /// Clear every listed Inbox row. Holds the count the dialog was opened with
    /// so the prompt states what it is about to do; the rows themselves are read
    /// again on confirmation.
    ConfirmInboxDismissAll {
        count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForceKillTarget {
    repo_path: PathBuf,
    agent_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewPane {
    Info,
    Claude,
    Diff,
    Terminal,
    /// Detail of a worktree failure the operator has not dismissed yet. Unlike
    /// the other tabs this one is not a fixed property of the selection type —
    /// see [`App::preview_panes`] for when it joins the tab list.
    Error,
}

/// Preview tabs a tree selection always has, in display order. The first entry
/// is the default tab used when nothing has been remembered yet. State-dependent
/// tabs are added on top of this by [`App::preview_panes`], which is what the
/// tab bar and tab cycling read — this list alone is not the whole tab bar.
pub(crate) fn panes_for(selection: Option<Selection>) -> &'static [PreviewPane] {
    match selection {
        Some(Selection::OverseerCategory(_)) => &[PreviewPane::Info, PreviewPane::Claude],
        // The inbox row is acted on from the left frame, so its preview is the
        // Inbox listing itself and there is no second tab to cycle to.
        Some(Selection::OverseerInbox(_)) => &[PreviewPane::Info],
        Some(Selection::Repo(_)) => &[
            PreviewPane::Info,
            PreviewPane::Claude,
            PreviewPane::Terminal,
        ],
        Some(Selection::Agent { .. }) => &[
            PreviewPane::Claude,
            PreviewPane::Info,
            PreviewPane::Diff,
            PreviewPane::Terminal,
        ],
        Some(Selection::ChildWorktree { .. }) => &[PreviewPane::Info, PreviewPane::Diff],
        Some(Selection::Orphan(_)) => &[PreviewPane::Claude],
        Some(Selection::OtherHeader) | Some(Selection::OrphanHeader) | None => &[],
    }
}

fn default_pane(selection: Option<Selection>) -> PreviewPane {
    panes_for(selection)
        .first()
        .copied()
        .unwrap_or(PreviewPane::Claude)
}

pub struct App {
    pub(crate) registry: Registry,
    pub(crate) config: Config,
    pub(crate) ephemeral_root: Option<PathBuf>,
    pub(crate) selected: usize,
    pub(crate) expanded: Vec<bool>,
    /// Stable worktree-path keys for agents whose child rows are expanded.
    /// Absence means collapsed, including for newly discovered owners.
    expanded_children: HashSet<String>,
    overseer_visible: bool,
    overseer_expanded: [bool; 4],
    /// Whether the "other locations" section (off-launch-dir repos that still
    /// have agents) is collapsed to its header row.
    other_collapsed: bool,
    /// Live robco-prefixed tmux sessions nothing in the registry accounts for.
    /// Runtime only; rebuilt by [`App::refresh_orphans`] on the discovery tick.
    orphans: Vec<crate::model::OrphanSession>,
    /// Whether the "orphan sessions" section is collapsed to its header row.
    orphans_collapsed: bool,
    pub(crate) preview: PreviewPane,
    /// Remembers the selected preview tab per tree item so switching selection
    /// restores the tab the user last viewed for that item. Keyed by repo path
    /// (repos) or agent id (agents) via [`App::item_key`].
    preview_tabs: HashMap<String, PreviewPane>,
    pub(crate) preview_scroll: u16,
    pub(crate) started: Instant,
    force_redraw: bool,
    mode: Mode,
    message: Option<(String, Instant)>,
    /// In-flight merges, keyed by repository path. Merging is serialised within
    /// a repository but not across repositories, so several entries at once is
    /// the normal case when the operator manages more than one repository.
    merge_jobs: HashMap<PathBuf, actions::merge::MergeJob>,
    /// Last merge result per repository, held until the operator dismisses it.
    /// Keyed the same way so one repository's result cannot overwrite another's.
    merge_outcomes: HashMap<PathBuf, actions::merge::MergeOutcome>,
    pr_precheck_job: Option<actions::pr_precheck::PrPrecheckJob>,
    clone_job: Option<actions::clone::CloneJob>,
    dropr_task_refresh: DroprTaskRefresh,
    background_refresh: BackgroundRefresh,
    preview_capture: PreviewCapture,
    /// Aggregated inbox, newest first. The rows the operator moves between are
    /// [`Selection::OverseerInbox`] entries into this list; it carries no cursor
    /// of its own.
    overseer_inbox: Vec<inbox::InboxItem>,
    /// Identity of every item the last aggregation derived, including the ones a
    /// dismissal is hiding. Pruning the dismissal list is done against this, not
    /// against `overseer_inbox` — see [`inbox::Inbox::targets`].
    overseer_inbox_targets: HashSet<(String, String)>,
    /// Overseer state (ledger, decisions, daemon liveness) captured off-thread by
    /// the background status worker. The overseer frame and previews render from
    /// this instead of reading disk on every draw.
    overseer_snapshot: overseer::OverseerSnapshot,
}

impl App {
    #[cfg(test)]
    pub fn new(registry: Registry, config: Config, launch_dir: PathBuf) -> Self {
        Self::new_with_ephemeral(registry, config, Some(launch_dir))
    }

    pub fn new_with_ephemeral(
        registry: Registry,
        config: Config,
        ephemeral_root: Option<PathBuf>,
    ) -> Self {
        let expanded = vec![true; registry.repos.len()];
        let overseer_visible = list::overseer_is_visible();
        let mut app = Self {
            registry,
            config,
            ephemeral_root,
            selected: 0,
            expanded,
            expanded_children: HashSet::new(),
            overseer_visible,
            overseer_expanded: [false; 4],
            other_collapsed: false,
            orphans: Vec::new(),
            orphans_collapsed: false,
            preview: PreviewPane::Info,
            preview_tabs: HashMap::new(),
            preview_scroll: 0,
            started: Instant::now(),
            force_redraw: false,
            mode: Mode::Normal,
            message: None,
            merge_jobs: HashMap::new(),
            merge_outcomes: HashMap::new(),
            pr_precheck_job: None,
            clone_job: None,
            dropr_task_refresh: DroprTaskRefresh::new(),
            background_refresh: BackgroundRefresh::new(),
            preview_capture: PreviewCapture::new(),
            overseer_inbox: Vec::new(),
            overseer_inbox_targets: HashSet::new(),
            overseer_snapshot: overseer::OverseerSnapshot::default(),
        };
        if app.prune_unmanaged_agents() {
            // Re-run the prune against the on-disk registry rather than writing
            // this snapshot back, so a worker another process registered while
            // robco was starting survives the startup save.
            let worktree_root = app.config.worktree_root.clone();
            let _ = app.locked_registry_update(|registry| {
                actions::discovery::prune_unmanaged(&mut registry.repos, &worktree_root);
            });
        }
        // A config whose merge strategy was migrated no longer reads the way it
        // behaves, and the merge key is the operator's to press — so the banner
        // says so before they press it.
        if let Some(notice) = app.config.merge_strategy_notice.clone() {
            app.show_message(notice);
        }
        app.refresh_orphans();
        app.restore_preview();
        app
    }

    pub(in crate::ui) fn overseer_category_expanded(&self, category: OverseerCategory) -> bool {
        self.overseer_expanded[category.index()]
    }

    pub(in crate::ui) fn overseer_frame_height(&self) -> u16 {
        if !self.overseer_visible {
            return 0;
        }
        layout::overseer_frame_height(tree::overseer_frame::content_lines(self).lines.len())
    }

    fn show_message(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }
}

fn suspend_terminal(action: impl FnOnce() -> Result<()>) -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen)?;
    let result = action();
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    enable_raw_mode()?;
    result
}
