use std::{
    collections::HashMap,
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
    Result, agent,
    config::Config,
    model::{OverseerCategory, Selection},
    registry::Registry,
    status,
};

use actions::dropr_tasks::DroprTaskRefresh;

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
mod inbox;
mod input;
mod input_wrap;
mod layout;
mod list;
mod merge_dialog;
mod overseer;
mod preview;
mod repo_description;
mod scrollback;
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
        with_prompt: bool,
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
    ConfirmRemoveRepo {
        path: PathBuf,
    },
    ConfirmMerge {
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
}

/// Preview tabs available for a given tree selection, in display order. The
/// first entry is the default tab used when nothing has been remembered yet.
pub(crate) fn panes_for(selection: Option<Selection>) -> &'static [PreviewPane] {
    match selection {
        Some(Selection::Overseer | Selection::OverseerCategory(_)) => {
            &[PreviewPane::Info, PreviewPane::Claude]
        }
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
    overseer_visible: bool,
    overseer_collapsed: bool,
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
    merge_job: Option<actions::merge::MergeJob>,
    merge_outcome: Option<actions::merge::MergeOutcome>,
    clone_job: Option<actions::clone::CloneJob>,
    dropr_task_refresh: DroprTaskRefresh,
    overseer_inbox: Vec<inbox::InboxItem>,
    overseer_inbox_selected: usize,
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
            overseer_visible,
            overseer_collapsed: false,
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
            merge_job: None,
            merge_outcome: None,
            clone_job: None,
            dropr_task_refresh: DroprTaskRefresh::new(),
            overseer_inbox: Vec::new(),
            overseer_inbox_selected: 0,
        };
        if app.prune_unmanaged_agents() {
            let _ = app.registry.save();
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

    fn tick(&mut self) {
        self.refresh_overseer_visibility();
        let prefix = self.config.tmux_session_prefix.clone();
        let processes = self
            .config
            .process_indicator
            .then(status::proc::ProcSnapshot::capture)
            .and_then(Result::ok);
        for repo in &mut self.registry.repos {
            let main_session = agent::repo_claude_session_name(&prefix, repo);
            status::refresh_repo_main(&main_session, repo, processes.as_ref());
            for agent in &mut repo.agents {
                status::refresh_agent(
                    &repo.path,
                    agent,
                    self.config.auto_accept,
                    processes.as_ref(),
                );
            }
        }
        let ledger = crate::overseer::ledger::Ledger::load().unwrap_or_default();
        let decisions = crate::overseer::logging::tail(200).unwrap_or_default();
        let reports = inbox::question_reports(&self.registry);
        self.overseer_inbox = inbox::aggregate(&ledger, &decisions, &reports);
        self.overseer_inbox_selected = self
            .overseer_inbox_selected
            .min(self.overseer_inbox.len().saturating_sub(1));
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
