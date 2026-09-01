use std::{
    collections::{HashMap, HashSet},
    io,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use crate::{Result, config::Config, locale::Locale, registry::Registry};

use actions::{
    background_refresh::BackgroundRefresh, dropr_tasks::DroprTaskRefresh,
    preview_capture::PreviewCapture, remote_hosts::HostSlot,
};
use backend::{Backend, LocalBackend};

/// How often the launch directory and each repo's worktrees are re-scanned to
/// pick up projects or worktrees created outside robco.
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(3);

mod actions;
mod backend;
mod blockfont;
mod confirm_pr;
#[cfg(test)]
mod confirm_pr_tests;
mod dialog;
mod dropr_task_focus;
mod error_dialog;
mod event_loop;
mod expand;
mod help;
mod hyperlink;
pub(crate) mod inbox;
mod input;
mod input_wrap;
mod layout;
mod list;
mod merge_dialog;
mod mode;
mod overseer;
mod preview;
mod preview_pane;
mod registry_write;
mod reorder;
mod repo_description;
mod scrollback;
#[cfg(test)]
mod sidebar_frame_tests;
pub(crate) mod spinner;
mod summary;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod text_input;
mod text_width;
mod theme;
mod tree;
mod ui_state;

use dropr_task_focus::DroprTaskFocus;
pub(crate) use mode::LandPlan;
use mode::{ForceKillTarget, Mode};
pub use preview_pane::PreviewPane;
use preview_pane::default_pane;
pub(crate) use preview_pane::panes_for;
use ui_state::UiStateStore;

pub use event_loop::run;

pub struct App {
    pub(crate) registry: Registry,
    pub(crate) config: Config,
    /// Resolved once from `config.language` at construction time — see
    /// `crate::locale::Locale::resolve`. Every localized render call site
    /// reads this instead of re-resolving `config.language` itself.
    pub(crate) locale: Locale,
    pub(crate) ephemeral_root: Option<PathBuf>,
    pub(crate) selected: usize,
    pub(crate) expanded: Vec<bool>,
    /// Stable worktree-path keys for agents whose child rows are expanded.
    /// Absence means collapsed, including for newly discovered owners.
    expanded_children: HashSet<String>,
    overseer_visible: bool,
    overseer_expanded: [bool; crate::model::OverseerCategory::COUNT],
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
    /// Level of the dropr task drill-down currently focused, when any. See
    /// [`DroprTaskFocus`].
    pub(crate) dropr_task_focus: Option<DroprTaskFocus>,
    pub(crate) started: Instant,
    force_redraw: bool,
    mode: Mode,
    message: Option<(String, Instant)>,
    /// In-flight merges, keyed by repository path. Merging is serialised within
    /// a repository but not across repositories, so several entries at once is
    /// the normal case when the operator manages more than one repository.
    merge_jobs: HashMap<PathBuf, actions::merge::MergeJob>,
    /// Merge approvals robco has queued to the daemon and the ledger has not
    /// confirmed yet, keyed by agent id (dropr:545). Only ever the hand-off
    /// window: `merge_queued` reads the ledger once the daemon has the
    /// approval. See `actions::merge_queued`.
    queued_merge_approvals: HashMap<String, actions::merge_queued::QueuedApproval>,
    /// Last merge result per repository, held until the operator dismisses it.
    /// Keyed the same way so one repository's result cannot overwrite another's.
    merge_outcomes: HashMap<PathBuf, actions::merge::MergeOutcome>,
    pr_precheck_job: Option<actions::pr_precheck::PrPrecheckJob>,
    /// dropr task launches currently in flight, keyed by the dropr task id
    /// (dropr:517). Several can run at once — the operator firing `n` down a
    /// list is the whole point — but the key is what stops a second `n` on
    /// the *same* row from starting a duplicate worker; a different row's
    /// `n` is never blocked by this map (dropr:508 kept a single global
    /// slot, which this now replaces).
    task_launch_jobs: HashMap<String, actions::dropr_task_worker::TaskLaunchJob>,
    clone_job: Option<actions::clone::CloneJob>,
    dropr_task_refresh: DroprTaskRefresh,
    /// Workspaces whose dropr task list a merge robco just finished has
    /// invalidated. Drained once per tick by
    /// `actions::dropr_task_settle` (dropr:510).
    dropr_task_settle: Vec<String>,
    backend: Arc<dyn Backend>,
    /// Independently-polled remote hosts, in configured tree order.
    hosts: Vec<HostSlot>,
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
    /// Sidebar layout the operator arranged, and its file. Every expand /
    /// collapse setter writes through this, so the layout survives a restart.
    ui_state: UiStateStore,
}

impl App {
    #[cfg(test)]
    pub fn new(registry: Registry, config: Config, launch_dir: PathBuf) -> Self {
        Self::build(
            registry,
            config,
            Some(launch_dir),
            UiStateStore::in_memory(ui_state::UiState::default()),
        )
    }

    pub fn new_with_ephemeral(
        registry: Registry,
        config: Config,
        ephemeral_root: Option<PathBuf>,
    ) -> Self {
        Self::build(registry, config, ephemeral_root, UiStateStore::load())
    }

    /// An app restored from a specific saved layout, for exercising what a
    /// restart sees without touching the operator's real state file.
    #[cfg(test)]
    pub(in crate::ui) fn new_with_ui_state(
        registry: Registry,
        config: Config,
        launch_dir: PathBuf,
        ui_state: UiStateStore,
    ) -> Self {
        Self::build(registry, config, Some(launch_dir), ui_state)
    }

    fn build(
        registry: Registry,
        config: Config,
        ephemeral_root: Option<PathBuf>,
        ui_state: UiStateStore,
    ) -> Self {
        let saved = ui_state.state();
        let expanded = saved.repo_expanded(&registry.repos);
        let expanded_children = saved.expanded_children.iter().cloned().collect();
        let overseer_expanded = saved.overseer_expanded();
        let other_collapsed = saved.other_collapsed;
        let orphans_collapsed = saved.orphans_collapsed;
        let overseer_visible = list::overseer_is_visible();
        let locale = Locale::resolve(config.language.as_deref());
        let mut app = Self {
            registry,
            config,
            locale,
            ephemeral_root,
            selected: 0,
            expanded,
            expanded_children,
            overseer_visible,
            overseer_expanded,
            other_collapsed,
            orphans: Vec::new(),
            orphans_collapsed,
            preview: PreviewPane::Info,
            preview_tabs: HashMap::new(),
            preview_scroll: 0,
            dropr_task_focus: None,
            started: Instant::now(),
            force_redraw: false,
            mode: Mode::Normal,
            message: None,
            merge_jobs: HashMap::new(),
            queued_merge_approvals: HashMap::new(),
            merge_outcomes: HashMap::new(),
            pr_precheck_job: None,
            task_launch_jobs: HashMap::new(),
            clone_job: None,
            dropr_task_refresh: DroprTaskRefresh::new(),
            dropr_task_settle: Vec::new(),
            backend: Arc::new(LocalBackend),
            hosts: Vec::new(),
            background_refresh: BackgroundRefresh::new(),
            preview_capture: PreviewCapture::new(),
            overseer_inbox: Vec::new(),
            overseer_inbox_targets: HashSet::new(),
            overseer_snapshot: overseer::OverseerSnapshot::default(),
            ui_state,
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
