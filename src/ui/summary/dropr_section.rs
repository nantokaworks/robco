//! The DROPR block of a repository summary — split out as its own section
//! module alongside its siblings (`checkout_state`, `history`, `other_prs`)
//! after dropr:470 grew it a `selected_task` parameter.

use ratatui::text::{Line, Span};

use crate::{
    locale::{Locale, t},
    model::RepoNode,
    ui::theme::DEFAULT as THEME,
};

use super::dropr_tasks::dropr_task_lines;

/// Always rendered. An unlinked repo used to drop the block entirely, which
/// looks identical to a linked repo whose task list happens to be empty — so
/// the operator could not tell "no tasks" from "robco never found a workspace
/// for this repo".
pub(super) fn dropr_section(
    repo: &RepoNode,
    width: u16,
    locale: Locale,
    selected_task: Option<usize>,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "─".repeat(usize::from(width)),
            THEME.muted_style(),
        )),
        Line::from(Span::styled("DROPR", THEME.accent_style())),
    ];
    // The drill-down's Level 1 (dropr:475): the operator has moved focus off
    // the repository tree and into this task list, so `j`/`k` walk tasks
    // instead of repos until `esc` steps back out. Said explicitly so the
    // next keypress is never a guess.
    if selected_task.is_some() {
        lines.push(Line::from(Span::styled(
            t(
                locale,
                "task list focused — j/k move, enter opens, esc/h/left back",
            ),
            THEME.hint_style(),
        )));
    }
    let Some(dropr) = &repo.dropr else {
        lines.push(Line::from(Span::styled(
            t(
                locale,
                "no workspace resolved for this repo, so no tasks can be listed",
            ),
            THEME.muted_style(),
        )));
        return lines;
    };
    let field = |name: &str, value: String| {
        Line::from(vec![
            Span::styled(format!("{name}: "), THEME.muted_style()),
            Span::raw(value),
        ])
    };
    lines.extend([
        field("kind", dropr.kind.clone()),
        field("id", dropr.id.clone()),
        field("name", dropr.name.clone()),
    ]);
    if !dropr.is_materialised() {
        // A virtual workspace has no task board behind it: the dispatch loop
        // skips this repo quietly (`overseer::dispatch::decision_log`), and
        // `ui::actions::dropr_tasks` never asks it for tasks either — asking
        // would only ever get "not found" back (dropr:516). One sentence
        // here covers both, so the task list below is never rendered for a
        // fetch that was never attempted.
        lines.push(Line::from(Span::styled(
            t(
                locale,
                "workspace is not materialised — no board exists yet, so no tasks are dispatched or listed for this repo",
            ),
            THEME.muted_style(),
        )));
        return lines;
    }
    lines.extend(dropr_task_lines(&repo.dropr_tasks, locale, selected_task));
    lines
}
