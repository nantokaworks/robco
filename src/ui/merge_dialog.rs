use ratatui::text::{Line, Span};

use crate::model::Selection;

use super::{App, actions::merge::MergeOutcome, spinner, theme::DEFAULT as THEME};

fn selected_agent(app: &App, selection: Option<Selection>) -> Option<(&std::path::Path, &str)> {
    if let Some(Selection::Agent { repo, agent }) = selection {
        let repo = app.registry.repos.get(repo)?;
        let agent = repo.agents.get(agent)?;
        Some((repo.path.as_path(), agent.id.as_str()))
    } else {
        None
    }
}

/// The selected agent's merge failure, while it is still unacknowledged. This is
/// what puts the error tab in the preview tab bar; a successful outcome and an
/// in-flight merge are transient and stay on the overlay instead.
fn unacknowledged_failure(app: &App, selection: Option<Selection>) -> Option<&MergeOutcome> {
    let (repo_path, agent_id) = selected_agent(app, selection)?;
    app.merge_outcome(repo_path)
        .filter(|outcome| outcome.agent_id == agent_id && outcome.result.is_err())
}

pub(in crate::ui) fn has_error(app: &App, selection: Option<Selection>) -> bool {
    unacknowledged_failure(app, selection).is_some()
}

/// Overlay content: the merge that is running right now, or one that finished
/// cleanly. Failures deliberately do not appear here — they own a preview tab so
/// nothing has to be drawn over the tab bar to report them.
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
        .filter(|outcome| outcome.agent_id == agent_id && outcome.result.is_ok())
    else {
        return Vec::new();
    };
    let mut notice = vec![Line::from(Span::styled(
        "MERGE COMPLETE",
        THEME.accent_style(),
    ))];
    notice.extend(outcome_details(outcome));
    notice.push(Line::from(Span::styled("esc dismiss", THEME.hint_style())));
    notice
}

/// Content of the error preview tab: everything the failure overlay used to say,
/// rendered inside the pane so the tab bar above it stays readable.
pub(in crate::ui) fn error_lines(app: &App, selection: Option<Selection>) -> Vec<Line<'static>> {
    let Some(outcome) = unacknowledged_failure(app, selection) else {
        return Vec::new();
    };
    let mut lines = vec![Line::from(Span::styled(
        "MERGE FAILED",
        THEME.merge_failed_style(true),
    ))];
    lines.extend(outcome_details(outcome));
    if let Err(detail) = &outcome.result {
        lines.push(Line::from(""));
        lines.extend(detail.lines().map(|line| {
            Line::from(Span::styled(
                line.to_string(),
                THEME.merge_failed_style(false),
            ))
        }));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("esc dismiss", THEME.hint_style())));
    lines
}

fn outcome_details(outcome: &MergeOutcome) -> Vec<Line<'static>> {
    vec![
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
    ]
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
