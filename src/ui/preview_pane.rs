use crate::model::Selection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewPane {
    Info,
    Claude,
    Diff,
    Terminal,
    /// Detail of a worktree failure the operator has not dismissed yet. Unlike
    /// the other tabs this one is not a fixed property of the selection type —
    /// see [`crate::ui::App::preview_panes`] for when it joins the tab list.
    Error,
}

/// Preview tabs a tree selection always has, in display order. The first entry
/// is the default tab used when nothing has been remembered yet. State-dependent
/// tabs are added on top of this by [`crate::ui::App::preview_panes`], which is what the
/// tab bar and tab cycling read — this list alone is not the whole tab bar.
pub(crate) fn panes_for(selection: Option<Selection>) -> &'static [PreviewPane] {
    match selection {
        // The control AI is a row of its own now (dropr:370), so no category
        // row owns a session to show behind a second tab.
        Some(Selection::OverseerCategory(_)) => &[PreviewPane::Info],
        // The row is acted on from the left frame (Enter attaches, `i`
        // instructs), so its one tab is the live control session capture
        // itself and there is no second tab to cycle to.
        Some(Selection::OverseerAi | Selection::RemoteControlAi(_)) => &[PreviewPane::Info],
        // The inbox row is acted on from the left frame, so its preview is the
        // Inbox listing itself and there is no second tab to cycle to.
        Some(
            Selection::OverseerInbox(_)
            | Selection::OverseerAlert(_)
            | Selection::RepoEscalation { .. },
        ) => &[PreviewPane::Info],
        // The channel row is acted on from the left frame too (Enter attaches
        // the live turn), so its one tab mirrors that same tmux session —
        // see `scrollback::live_session`'s `Selection::DiscordChannel` arm.
        Some(Selection::DiscordChannel(_) | Selection::RemoteDiscordChannel { .. }) => {
            &[PreviewPane::Info]
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

pub(in crate::ui) fn default_pane(selection: Option<Selection>) -> PreviewPane {
    panes_for(selection)
        .first()
        .copied()
        .unwrap_or(PreviewPane::Claude)
}
