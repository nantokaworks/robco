//! The repo row's own INFO preview (dropr:475): its plain summary, or — while
//! the dropr task drill-down is focused — the task list highlighting its
//! cursor (Level 1) or one task's full body in its place (Level 2/3). Split
//! out of `ui::preview` to keep that file under this project's source file
//! size limit.

use std::path::Path;

use ratatui::text::Text;

use crate::{
    locale::Locale,
    model::RepoNode,
    overseer::{ledger::Ledger, other_prs::OtherPrs},
    ui::{
        DroprTaskFocus,
        summary::{dropr_task_body, repo_summary},
    },
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
) -> (String, Text<'static>) {
    // Falls back to the plain summary below when the focused task drifted
    // out of the list mid-read (e.g. it closed) rather than showing a body
    // for a task that is no longer there.
    if let Some(DroprTaskFocus::Body { task }) = focus
        && let Some(result) = dropr_task_body(repo, task, locale)
    {
        return result;
    }
    let selected_task = match focus {
        Some(DroprTaskFocus::List { task }) => Some(task),
        _ => None,
    };
    repo_summary(repo, repos_root, ledger, other_prs, width, locale, selected_task)
}
