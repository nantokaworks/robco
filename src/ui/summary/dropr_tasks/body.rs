//! Level 2 of the dropr task drill-down (dropr:475): one task's full body —
//! title, status, priority, description, and its subtasks — rendered in
//! place of the whole repo summary while `ui::DroprTaskFocus::Body` is
//! focused. See `ui::preview::draw`'s `DroprTaskFocus::Body` arm.

use ratatui::text::{Line, Span};

use crate::{
    locale::{Locale, t},
    model::RepoNode,
    ui::theme::DEFAULT as THEME,
};

use super::nesting;

/// Renders `task_index`'s body (as `super::selectable_tasks` orders them),
/// or `None` when the index no longer names a listed task — e.g. it closed
/// or dropped off the fetch since the body was opened.
pub(in crate::ui) fn render(
    repo: &RepoNode,
    task_index: usize,
    locale: Locale,
) -> Option<(String, ratatui::text::Text<'static>)> {
    let candidate = super::selectable_tasks(&repo.dropr_tasks)
        .get(task_index)
        .copied()?;

    let mut lines = vec![
        Line::from(Span::styled(
            t(
                locale,
                "task body — one key starts the work, esc/h/left steps back",
            ),
            THEME.accent_style(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            candidate.title.clone(),
            THEME.accent_bold_style(),
        )),
        Line::from(""),
    ];

    let field = |name: &str, value: String| {
        Line::from(vec![
            Span::styled(format!("{name}: "), THEME.muted_style()),
            Span::raw(value),
        ])
    };
    lines.push(field("id", candidate.display_id.clone()));
    lines.push(field("status", candidate.status.clone()));
    lines.push(field(
        "priority",
        if candidate.priority.is_empty() {
            "(none)".to_string()
        } else {
            candidate.priority.clone()
        },
    ));
    lines.push(Line::from(""));

    match candidate
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty())
    {
        Some(description) => {
            lines.extend(description.lines().map(|line| Line::from(line.to_string())))
        }
        None => lines.push(Line::from(Span::styled(
            t(locale, "(no description)"),
            THEME.muted_style(),
        ))),
    }
    lines.push(Line::from(""));

    // A section heading, the same chrome as "DROPR" / "next tasks" / "in
    // progress" — stays English in every locale (dropr:377).
    lines.push(Line::from(Span::styled("subtasks", THEME.accent_style())));
    if candidate.child_count == 0 {
        lines.push(Line::from(Span::styled(
            t(locale, "(no subtasks)"),
            THEME.muted_style(),
        )));
    } else if !repo
        .dropr_tasks
        .subtrees_known
        .contains(candidate.id.as_str())
    {
        lines.push(Line::from(Span::styled(
            t(locale, "subtasks were not fetched for this task"),
            THEME.muted_style(),
        )));
    } else {
        let children = nesting::children_by_parent(&repo.dropr_tasks.tasks);
        lines.extend(nesting::nested_lines(
            locale,
            candidate,
            &children,
            &repo.dropr_tasks.subtrees_known,
        ));
    }

    Some((
        format!("{} / {}", repo.name, candidate.display_id),
        lines.into(),
    ))
}

#[cfg(test)]
#[path = "body_tests.rs"]
mod tests;
