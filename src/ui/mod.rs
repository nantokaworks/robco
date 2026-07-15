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

use crate::{Result, agent, config::Config, model::Selection, registry::Registry, status};

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
mod input;
mod input_wrap;
mod layout;
mod list;
mod merge_dialog;
mod preview;
mod repo_description;
mod scrollback;
pub(crate) mod spinner;
mod summary;
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
    pub(crate) launch_dir: PathBuf,
    pub(crate) selected: usize,
    pub(crate) expanded: Vec<bool>,
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
    dropr_task_refresh: DroprTaskRefresh,
}

impl App {
    pub fn new(registry: Registry, config: Config, launch_dir: PathBuf) -> Self {
        let expanded = vec![true; registry.repos.len()];
        let mut app = Self {
            registry,
            config,
            launch_dir,
            selected: 0,
            expanded,
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
            dropr_task_refresh: DroprTaskRefresh::new(),
        };
        if app.prune_unmanaged_agents() {
            let _ = app.registry.save();
        }
        app.refresh_orphans();
        app.restore_preview();
        app
    }

    fn show_message(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }

    fn tick(&mut self) {
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::{config::Config, registry::Registry};

    fn test_app() -> App {
        let temp = tempfile::tempdir().unwrap();
        App::new(Registry::default(), Config::default(), temp.path().into())
    }

    #[test]
    fn visible_message_does_not_swallow_next_key() {
        let mut app = test_app();
        app.show_message("done");
        let quit = app
            .handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE))
            .unwrap();

        assert!(!quit);
        assert!(app.message.is_none());
        assert!(matches!(app.mode, Mode::Help { scroll: 0 }));
    }

    #[test]
    fn confirm_pr_y_and_n_edit_and_escape_cancels() {
        let mut app = test_app();
        app.mode = Mode::ConfirmPr {
            repo_path: "/repo".into(),
            agent_id: "agent".to_string(),
            branch: "feature/agent".to_string(),
            input: "prompt".to_string(),
        };

        for code in [KeyCode::Char('y'), KeyCode::Char('n'), KeyCode::Backspace] {
            app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
                .unwrap();
        }
        assert!(matches!(
            &app.mode,
            Mode::ConfirmPr { input, .. } if input == "prompty"
        ));

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(app.mode, Mode::Normal));
    }
}
