use ratatui::text::{Line, Span, Text};

use crate::model::Selection;

use super::{App, PreviewPane, spinner, theme::DEFAULT as THEME};

pub(super) fn preview_title(app: &App) -> Option<Line<'static>> {
    if let Some(job) = app.merge_job() {
        return Some(
            Line::from(Span::styled(
                format!(" MERGING {} · {} ", job.branch, job.step),
                THEME.accent_style(),
            ))
            .left_aligned(),
        );
    }

    app.merge_outcome().map(|outcome| {
        let status = if outcome.result.is_ok() {
            "MERGE COMPLETE"
        } else {
            "MERGE FAILED"
        };
        Line::from(Span::styled(
            format!(" {status} {} ", outcome.branch),
            THEME.accent_style(),
        ))
        .left_aligned()
    })
}

pub(super) fn append_preview(
    app: &App,
    pane: PreviewPane,
    selection: Option<Selection>,
    text: &mut Text<'_>,
) {
    if pane == PreviewPane::Info
        && let Some(Selection::Agent { repo, agent }) = selection
    {
        let repo = &app.registry.repos[repo];
        let agent = &repo.agents[agent];
        if let Some(job) = app.merge_job()
            && job.repo_path == repo.path
            && job.agent_id == agent.id
        {
            text.lines.splice(
                3..3,
                [
                    Line::from(vec![
                        Span::styled("merging: ", THEME.muted_style()),
                        Span::raw(job.branch.clone()),
                    ]),
                    Line::from(Span::styled(
                        format!("{} {}", spinner::frame(app.started.elapsed()), job.step),
                        THEME.accent_style(),
                    )),
                ],
            );
        }
    }

    let mut notice = Vec::new();
    if let Some(job) = app.merge_job() {
        notice.push(Line::from(vec![
            Span::styled("MERGING ", THEME.accent_style()),
            Span::raw(job.branch.clone()),
            Span::styled(
                format!("  {} {}", spinner::frame(app.started.elapsed()), job.step),
                THEME.accent_style(),
            ),
        ]));
        notice.push(Line::from(""));
    }
    if let Some(outcome) = app.merge_outcome() {
        notice.push(Line::from(Span::styled(
            match &outcome.result {
                Ok(()) => "MERGE COMPLETE",
                Err(_) => "MERGE FAILED",
            },
            THEME.accent_style(),
        )));
        notice.push(Line::from(vec![
            Span::styled("branch: ", THEME.muted_style()),
            Span::raw(outcome.branch.clone()),
        ]));
        notice.push(Line::from(vec![
            Span::styled("agent: ", THEME.muted_style()),
            Span::raw(outcome.agent_id.clone()),
        ]));
        notice.push(Line::from(vec![
            Span::styled("repository: ", THEME.muted_style()),
            Span::raw(outcome.repo_path.display().to_string()),
        ]));
        if let Err(detail) = &outcome.result {
            notice.extend(detail.lines().map(|line| Line::from(line.to_string())));
        }
        notice.push(Line::from(""));
    }
    text.lines.splice(0..0, notice);
}
