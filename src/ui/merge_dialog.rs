use ratatui::text::{Line, Span};

use crate::model::Selection;

use super::{App, spinner, theme::DEFAULT as THEME};

fn selected_agent(app: &App, selection: Option<Selection>) -> Option<(&std::path::Path, &str)> {
    if let Some(Selection::Agent { repo, agent }) = selection {
        let repo = app.registry.repos.get(repo)?;
        let agent = repo.agents.get(agent)?;
        Some((repo.path.as_path(), agent.id.as_str()))
    } else {
        None
    }
}

pub(in crate::ui) fn notice_lines(app: &App, selection: Option<Selection>) -> Vec<Line<'static>> {
    let Some((repo_path, agent_id)) = selected_agent(app, selection) else {
        return Vec::new();
    };

    if let Some(job) = app.merge_job(repo_path)
        && job.agent_id == agent_id
    {
        return vec![Line::from(vec![
            Span::styled("MERGING ", THEME.accent_style()),
            Span::raw(job.branch.clone()),
            Span::styled(
                format!("  {} {}", spinner::frame(app.started.elapsed()), job.step),
                THEME.accent_style(),
            ),
        ])];
    }

    let Some(outcome) = app
        .merge_outcome(repo_path)
        .filter(|outcome| outcome.agent_id == agent_id)
    else {
        return Vec::new();
    };
    let mut notice = vec![
        Line::from(Span::styled(
            if outcome.result.is_ok() {
                "MERGE COMPLETE"
            } else {
                "MERGE FAILED"
            },
            THEME.accent_style(),
        )),
        Line::from(vec![
            Span::styled("branch: ", THEME.muted_style()),
            Span::raw(outcome.branch.clone()),
        ]),
        Line::from(vec![
            Span::styled("agent: ", THEME.muted_style()),
            Span::raw(outcome.agent_id.clone()),
        ]),
        Line::from(vec![
            Span::styled("repository: ", THEME.muted_style()),
            Span::raw(outcome.repo_path.display().to_string()),
        ]),
    ];
    if let Err(detail) = &outcome.result {
        notice.extend(detail.lines().map(|line| Line::from(line.to_string())));
    }
    notice.push(Line::from(Span::styled("esc dismiss", THEME.hint_style())));
    notice
}

pub(in crate::ui) fn preview_title(
    app: &App,
    selection: Option<Selection>,
) -> Option<Line<'static>> {
    let (repo_path, agent_id) = selected_agent(app, selection)?;
    if let Some(job) = app.merge_job(repo_path)
        && job.agent_id == agent_id
    {
        return Some(
            Line::from(Span::styled(
                format!(" MERGING {} · {} ", job.branch, job.step),
                THEME.accent_style(),
            ))
            .left_aligned(),
        );
    }

    app.merge_outcome(repo_path)
        .filter(|outcome| outcome.agent_id == agent_id)
        .map(|outcome| {
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
