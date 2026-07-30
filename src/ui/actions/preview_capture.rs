//! Off-thread preview capture.
//!
//! Rendering the preview pane used to spawn tmux (`resize` + `capture-pane`) or
//! `git diff` subprocesses *synchronously inside the draw*, and the draw runs on
//! every event-loop iteration — after every keypress and on every idle tick. On
//! macOS a subprocess spawn costs milliseconds, so moving the selection through
//! the tree stalled behind a fresh capture per row. This module runs the capture
//! on a background worker (mirroring [`super::background_refresh`]) and lets the
//! draw read the last completed result, so the UI thread never blocks on a
//! subprocess.

use std::{
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use ansi_to_tui::IntoText;
use ratatui::{layout::Rect, text::Line, text::Text};

use crate::{
    git,
    locale::{Locale, t},
    model::{Selection, Status},
    ui::{App, PreviewPane, layout, scrollback},
};

/// How often the visible preview is re-captured while the selection is stable.
/// Fast enough to feel live, slow enough that idle ticks stay cheap.
const PREVIEW_INTERVAL: Duration = Duration::from_millis(250);

/// What the current selection + preview tab wants captured. `Tmux` mirrors a live
/// session at a specific inner size and scrollback `offset`; `Diff` renders a
/// worktree diff.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::ui) enum CaptureTarget {
    Tmux {
        session: String,
        width: u16,
        height: u16,
        offset: u16,
    },
    Diff {
        path: PathBuf,
    },
}

type CaptureMessage = (CaptureTarget, Option<Text<'static>>);

pub(in crate::ui) struct PreviewCapture {
    tx: Sender<CaptureMessage>,
    rx: Receiver<CaptureMessage>,
    /// Last completed capture. The inner `Option` is `None` when the target was
    /// captured but produced nothing (e.g. the tmux session is gone), which the
    /// draw path renders as its own placeholder.
    current: Option<(CaptureTarget, Option<Text<'static>>)>,
    in_flight: bool,
    last_target: Option<CaptureTarget>,
    last_at: Option<Instant>,
}

impl PreviewCapture {
    pub(in crate::ui) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            current: None,
            in_flight: false,
            last_target: None,
            last_at: None,
        }
    }

    /// Request a capture of `target`. Spawns a worker when the target changed or
    /// the refresh interval elapsed, unless one is already in flight — a single
    /// slot keeps a hung tmux/git call from piling up replacement workers.
    fn request(&mut self, target: CaptureTarget, locale: Locale) {
        if self.in_flight {
            return;
        }
        let changed = self.last_target.as_ref() != Some(&target);
        let due = self
            .last_at
            .is_none_or(|at| at.elapsed() >= PREVIEW_INTERVAL);
        if !changed && !due {
            return;
        }
        let sender = self.tx.clone();
        let job = target.clone();
        let spawn = thread::Builder::new()
            .name("ui-preview-capture".into())
            .spawn(move || {
                let text = panic::catch_unwind(AssertUnwindSafe(|| capture(&job, locale)))
                    .ok()
                    .flatten();
                let _ = sender.send((job, text));
            });
        if spawn.is_ok() {
            self.in_flight = true;
            self.last_target = Some(target);
            self.last_at = Some(Instant::now());
        }
    }

    fn ingest(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            self.in_flight = false;
            self.current = Some(message);
        }
    }
}

/// Produce the preview text for `target` off the UI thread.
fn capture(target: &CaptureTarget, locale: Locale) -> Option<Text<'static>> {
    match target {
        CaptureTarget::Tmux {
            session,
            width,
            height,
            offset,
        } => scrollback::capture_inner(session, *width, *height, *offset),
        CaptureTarget::Diff { path } => Some(
            git::diff(path)
                .unwrap_or_else(|err| err.to_string())
                .into_text()
                .unwrap_or_else(|_| vec![Line::from(t(locale, "Could not render diff."))].into()),
        ),
    }
}

impl App {
    /// Schedule a background capture for whatever the current selection needs.
    /// Called once per event-loop iteration with the full terminal area.
    pub(in crate::ui) fn schedule_preview_capture(&mut self, full_area: Rect) {
        if let Some(target) = self.current_capture_target(full_area) {
            self.preview_capture.request(target, self.locale);
        }
    }

    pub(in crate::ui) fn ingest_preview_captures(&mut self) {
        self.preview_capture.ingest();
    }

    /// The last completed capture of `session`, if the most recent capture was of
    /// that session. Returns `None` both when nothing is cached yet and when the
    /// session produced no output, so the caller's placeholder covers both.
    pub(in crate::ui) fn cached_tmux(&self, session: &str) -> Option<Text<'static>> {
        match &self.preview_capture.current {
            Some((
                CaptureTarget::Tmux {
                    session: cached, ..
                },
                text,
            )) if cached == session => text.clone(),
            _ => None,
        }
    }

    /// The last completed diff capture for `path`, if that is what is cached.
    pub(in crate::ui) fn cached_diff(&self, path: &std::path::Path) -> Option<Text<'static>> {
        match &self.preview_capture.current {
            Some((CaptureTarget::Diff { path: cached }, text)) if cached == path => text.clone(),
            _ => None,
        }
    }

    fn current_capture_target(&self, full_area: Rect) -> Option<CaptureTarget> {
        let panes = layout::panes(layout::root(full_area).body, self.overseer_frame_height());
        let selection = self.selected_item()?;
        // Diff tabs render a worktree diff rather than a live session.
        if self.preview == PreviewPane::Diff {
            match selection {
                Selection::Agent { repo, agent } => {
                    let agent = &self.registry.repos[repo].agents[agent];
                    if agent.status != Status::BranchOnly {
                        return Some(CaptureTarget::Diff {
                            path: agent.worktree_path.clone(),
                        });
                    }
                }
                Selection::ChildWorktree { repo, agent, child } => {
                    return Some(CaptureTarget::Diff {
                        path: self.registry.repos[repo].agents[agent].children[child]
                            .path
                            .clone(),
                    });
                }
                _ => {}
            }
        }
        // Live tmux tabs (CLAUDE / TERM / overseer control / orphan).
        let session = scrollback::live_session(self)?;
        let (width, height) = scrollback::inner_dims(panes.preview);
        if width == 0 || height == 0 {
            return None;
        }
        Some(CaptureTarget::Tmux {
            session,
            width,
            height,
            offset: self.preview_scroll,
        })
    }
}
