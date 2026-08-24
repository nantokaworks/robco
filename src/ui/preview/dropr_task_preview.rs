//! The repo row's own INFO preview (dropr:475): its plain summary, or — while
//! the dropr task list is focused — the same summary with the cursor
//! highlighted in its DROPR section. Split out of `ui::preview` to keep that
//! file under this project's source file size limit.
//!
//! Reading one task's full body used to swap this pane's whole render target
//! for a second, task-only view (`DroprTaskFocus::Body`); it is now
//! `Mode::TaskBody`, a dialog drawn over whatever this function renders
//! (dropr:501) — see `ui::dialog::task_body`. So this function always renders
//! the list; it has nothing left to fall back from.

use std::path::Path;

use ratatui::text::Text;

use crate::{
    locale::Locale,
    model::RepoNode,
    overseer::{ledger::Ledger, other_prs::OtherPrs},
    ui::{DroprTaskFocus, summary::repo_summary},
};

#[allow(clippy::too_many_arguments)]
pub(super) fn render(
    repo: &RepoNode,
    repos_root: &Path,
    ledger: &Ledger,
    other_prs: &OtherPrs,
    width: u16,
    locale: Locale,
    focus: Option<DroprTaskFocus>,
    dropr_fetch_in_flight: bool,
) -> (String, Text<'static>) {
    repo_summary(
        repo,
        repos_root,
        ledger,
        other_prs,
        width,
        locale,
        focus.map(|focus| focus.task),
        dropr_fetch_in_flight,
    )
}
