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

use crate::{Result, config::Config, locale::Locale, model::Selection, registry::Registry};

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
mod expand;
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

use text_input::TextInput;
use ui_state::UiStateStore;

pub use event_loop::run;

enum Mode {
    Normal,
    Help {
        scroll: u16,
    },
    PromptAgent {
        repo: usize,
        input: TextInput,
    },
    PromptRepo {
        input: TextInput,
    },
    PromptOverseer {
        input: TextInput,
    },
    /// The answer prompt carries the whole row it was opened for, not just the
    /// target session: on a successful send the row's `(kind, target_id, at)`
    /// identity is what marks it handled (`App::answer_inbox`), and the
    /// identity must be the one the operator was looking at, not whatever a
    /// later refresh re-derived under the prompt.
    PromptInbox {
        item: inbox::InboxItem,
        input: TextInput,
    },
    ConfirmKill {
        repo: usize,
        agent: usize,
    },
    ConfirmRemoveRepo {
        path: PathBuf,
    },
    ConfirmMerge {
        repo: usize,
        agent: usize,
        plan: LandPlan,
        head: Option<String>,
    },
    /// The agent's pull request already merged, so `m` offers the cleanup that
    /// normally follows a merge instead of a merge that has nothing left to do.
    ConfirmCleanup {
        repo: usize,
        agent: usize,
    },
    /// Cancellable progress modal shown while `PrPrecheckJob` runs in the
    /// background, so pressing P never flashes `ConfirmPr` open only to have
    /// the precheck close it again a frame later.
    PrPrecheck {
        repo_path: PathBuf,
        agent_id: String,
        branch: String,
        approval_head: Option<String>,
    },
    ConfirmPr {
        repo_path: PathBuf,
        agent_id: String,
        branch: String,
        input: TextInput,
        approval_head: Option<String>,
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
    // Panic-stop the overseer: kill every overseer-managed worker. Reachable
    // only while an OVERSEER row is selected.
    ConfirmOverseerPanic,
    /// Durably stop the Overseer daemon process itself (launchd bootout, or a
    /// manual SIGTERM for a daemon started with `robco overseer run`) — unlike
    /// `ConfirmOverseerPanic`, this ends the daemon process, not just its
    /// workers. Reachable only while the overseer panel is visible and the
    /// daemon is alive.
    ConfirmDaemonStop,
    /// Clear every listed Inbox row. Holds the count the dialog was opened with
    /// so the prompt states what it is about to do; the rows themselves are read
    /// again on confirmation.
    ConfirmInboxDismissAll {
        count: usize,
    },
    /// Read-only view of one dropr task's full body (dropr:501), opened by
    /// `Enter` on a task-list row while `DroprTaskFocus` is focused. Drawn as
    /// a dialog (`ui::dialog::task_body`) over the task list, which stays
    /// untouched underneath — closing this (`Esc`/`h`/`Left`) returns to the
    /// exact list cursor and scroll position it had before the body opened.
    /// `scroll` is this dialog's own paragraph scroll, independent of the
    /// list pane's `preview_scroll`.
    TaskBody {
        task: usize,
        scroll: u16,
    },
    /// Delete a retained Discord channel record. Holds the channel id (not an
    /// index) since the row order re-derives from `last_active_at` on every
    /// refresh — the same hazard `ConfirmKillOrphan` guards against. `label`
    /// is the display label the dialog was opened with, reused for the
    /// result message so it reads the same even if the record is gone by the
    /// time the operator confirms.
    ConfirmRemoveDiscordChannel {
        channel_id: String,
        label: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LandPlan {
    MergeNow,
    QueueApproval,
    OpenPrThenQueue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForceKillTarget {
    repo_path: PathBuf,
    agent_id: String,
}

/// Focus inside a repository's dropr task-list drill-down, entered from
/// `Selection::Repo` with the INFO pane showing (dropr:475). Task rows are no
/// longer members of the outer cursor list — `App::selected` stays on the
/// repository row the whole time this is `Some`, and movement keys are
/// intercepted (`ui::input::dropr_task_drill::handle_normal`) to walk this
/// instead. `task` indexes into the same
/// `ui::summary::dropr_tasks::selectable_tasks` order `ui::actions`'s
/// dropr-task modules (walking the list, opening a body, launching it) read
/// this same list.
///
/// Reading one task's full body used to be a second state here
/// (`DroprTaskFocus::Body`); it is now `Mode::TaskBody`, a dialog drawn over
/// this list instead of a state that replaces it (dropr:501) — this cursor
/// never changes while that dialog is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DroprTaskFocus {
    pub(crate) task: usize,
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
        // The control AI is a row of its own now (dropr:370), so no category
        // row owns a session to show behind a second tab.
        Some(Selection::OverseerCategory(_)) => &[PreviewPane::Info],
        // The row is acted on from the left frame (Enter attaches, `i`
        // instructs), so its one tab is the live control session capture
        // itself and there is no second tab to cycle to.
        Some(Selection::OverseerAi) => &[PreviewPane::Info],
        // The inbox row is acted on from the left frame, so its preview is the
        // Inbox listing itself and there is no second tab to cycle to.
        Some(Selection::OverseerInbox(_)) => &[PreviewPane::Info],
        // The channel row is acted on from the left frame too (Enter attaches
        // the live turn), so its one tab mirrors that same tmux session —
        // see `scrollback::live_session`'s `Selection::DiscordChannel` arm.
        Some(Selection::DiscordChannel(_)) => &[PreviewPane::Info],
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
            merge_outcomes: HashMap::new(),
            pr_precheck_job: None,
            clone_job: None,
            dropr_task_refresh: DroprTaskRefresh::new(),
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
