//! The repo row: name, agent count, dropr/indicator glyphs, and — when
//! collapsed — a rollup of its agents' statuses. Split out of `tree::draw`
//! because the repo row alone carries as much rendering logic as every other
//! row kind combined, and `#306` (repo-level Overseer opt-out) added its own
//! marker and dimming on top of that.

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::model::{ManagementMode, Status};
use crate::ui::{App, theme::DEFAULT as THEME};

use super::indicator::{self, IndicatorState, select, select_supplementary};
use super::label;

/// The repo row itself, plus a trailing "(no agents)" filler line when the
/// repo is expanded and empty.
pub(super) fn build(
    app: &App,
    repo_idx: usize,
    selected: bool,
    marker: &str,
    style: Style,
    projects_width: u16,
) -> Vec<Line<'static>> {
    let repo = &app.registry.repos[repo_idx];
    let expanded = app.expanded.get(repo_idx).copied().unwrap_or(true);
    let prefix = app.config.project_icon.marker(expanded);
    // Opted out of the Overseer entirely (`G` key): dim the row so the whole
    // subtree reads as hands-off at a glance, without hunting for the
    // one-cell marker that follows.
    let unmanaged = repo.management == ManagementMode::Manual;
    let title_style = if selected {
        style
    } else if unmanaged {
        THEME.muted_style()
    } else {
        style
    };
    let mut right = vec![Span::styled(
        format!(" {}", repo.agents.len()),
        if selected { style } else { THEME.hint_style() },
    )];
    if !app.repo_is_local(repo) {
        right.push(Span::styled(
            format!("  {}", super::short_path(&repo.path)),
            if selected { style } else { THEME.muted_style() },
        ));
    }
    let dropr_refresh = repo
        .dropr
        .as_ref()
        .is_some_and(|workspace| app.dropr_refresh_in_flight(&workspace.id));
    let mut indicator_state = IndicatorState::with_status(repo.main_status);
    indicator_state.shell_active = repo.main_shell_working;
    indicator_state.mcp_active = repo.main_mcp_active;
    indicator_state.subagents_active = repo.main_subagents_active;
    indicator_state.dropr_refresh = dropr_refresh;
    let primary = select(indicator_state);
    right.extend(indicator::supplementary_spans(
        primary,
        select_supplementary(indicator_state),
        selected,
        "  ",
    ));
    if !expanded && !repo.agents.is_empty() {
        collapsed_rollup(repo, selected, style, &mut right);
    }
    let mut lines = vec![label::labeled_row(
        projects_width,
        vec![
            Span::styled(format!("{marker} {prefix} "), style),
            label::repo_management_glyph(repo.management, THEME.management_marker_style(selected)),
            Span::styled(" ", style),
        ],
        primary,
        &repo.name,
        title_style,
        selected,
        app.started.elapsed(),
        right,
    )];
    if expanded && repo.agents.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("    {}(no agents)", label::AGENT_INDENT),
            THEME.muted_style(),
        )));
    }
    lines
}

/// Status-glyph rollup shown on a collapsed repo row in place of its
/// (invisible) agent rows.
fn collapsed_rollup(
    repo: &crate::model::RepoNode,
    selected: bool,
    style: Style,
    right: &mut Vec<Span<'static>>,
) {
    let status_counts = [
        Status::Running,
        Status::Waiting,
        Status::Done,
        Status::Idle,
        Status::Dead,
        Status::BranchOnly,
    ]
    .map(|status| {
        (
            status,
            repo.agents
                .iter()
                .filter(|agent| agent.status == status)
                .count(),
        )
    });
    let status_style = |status| {
        if selected {
            THEME.selected_status_style(status)
        } else {
            THEME.status_style(status)
        }
    };
    let mut first = true;
    for (status, count) in status_counts {
        if count == 0 {
            continue;
        }
        right.push(Span::styled(if first { "  " } else { " · " }, style));
        right.push(Span::styled(
            format!("{count} {}", status.glyph()),
            status_style(status),
        ));
        first = false;
    }
    let missing_count = repo
        .agents
        .iter()
        .filter(|agent| agent.worktree_missing)
        .count();
    if missing_count > 0 {
        right.push(Span::styled(if first { "  " } else { " · " }, style));
        right.push(Span::styled(
            format!("{missing_count} ⌦"),
            THEME.worktree_missing_style(selected),
        ));
    }
    let merge_failed_count = repo
        .agents
        .iter()
        .filter(|agent| agent.merge_error.is_some())
        .count();
    if merge_failed_count > 0 {
        right.push(Span::styled(if first { "  " } else { " · " }, style));
        right.push(Span::styled(
            format!("{merge_failed_count} merge-failed"),
            THEME.merge_failed_style(selected),
        ));
    }
}
