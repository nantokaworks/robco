use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::model::{Selection, Status};
use crate::subagents::SubagentStatus;

use super::{layout, theme::DEFAULT as THEME, App};
use indicator::{select, select_supplementary, IndicatorState};

mod footer;
mod hints;
mod indicator;
mod label;
mod overseer_frame;

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection], message: Option<&str>) {
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body, app.overseer_visible);
    if app.overseer_visible {
        overseer_frame::draw(frame, app, panes.overseer);
    }
    let projects_width = panes.tree.width.saturating_sub(2);

    let mut lines = Vec::new();
    for (idx, item) in visible.iter().enumerate() {
        let selected = idx == app.selected;
        let marker = if selected { ">" } else { " " };
        let style = if selected {
            THEME.selection_style()
        } else {
            THEME.accent_style()
        };

        match *item {
            Selection::Overseer => continue,
            Selection::Repo(repo_idx) => {
                let repo = &app.registry.repos[repo_idx];
                let expanded = app.expanded.get(repo_idx).copied().unwrap_or(true);
                let prefix = app.config.project_icon.marker(expanded);
                let mut right = vec![Span::styled(
                    format!(" {}", repo.agents.len()),
                    if selected { style } else { THEME.hint_style() },
                )];
                if !app.repo_is_local(repo) {
                    right.push(Span::styled(
                        format!("  {}", short_path(&repo.path)),
                        if selected { style } else { THEME.muted_style() },
                    ));
                }
                let dropr_refresh = repo
                    .dropr
                    .as_ref()
                    .is_some_and(|workspace| app.dropr_refresh_in_flight(&workspace.id));
                let mut indicator_state = IndicatorState::with_status(repo.main_status);
                indicator_state.shell_active = repo.main_shell_working;
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
                lines.push(label::labeled_row(
                    projects_width,
                    format!("{marker} {prefix} "),
                    primary,
                    &repo.name,
                    style,
                    style,
                    selected,
                    app.started.elapsed(),
                    right,
                ));
                if expanded && repo.agents.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "    (no agents)",
                        THEME.muted_style(),
                    )));
                }
            }
            Selection::Agent { repo, agent } => {
                let repo = &app.registry.repos[repo];
                let depth = crate::model::agent_depth(&repo.agents, agent);
                let agent = &repo.agents[agent];
                let agent_style = if selected {
                    style
                } else if agent.status == Status::BranchOnly {
                    THEME.status_style(Status::BranchOnly)
                } else {
                    style
                };
                let active = agent
                    .subagents
                    .iter()
                    .filter(|subagent| subagent.status == SubagentStatus::Running)
                    .count();
                let mut indicator_state = IndicatorState::with_status(Some(agent.status));
                indicator_state.merging = app.is_merging_agent(&repo.path, &agent.id);
                indicator_state.worktree_missing = agent.worktree_missing;
                indicator_state.merge_failed = agent.merge_error.is_some();
                indicator_state.shell_active = agent.shell_working;
                indicator_state.subagents_active = active;
                if crate::overseer::is_overseer_child(agent.parent_agent_id.as_deref()) {
                    indicator_state.management = Some(agent.management);
                }
                let primary = select(indicator_state);
                let right = indicator::supplementary_spans(
                    primary,
                    select_supplementary(indicator_state),
                    selected,
                    " ",
                );
                lines.push(label::labeled_row(
                    projects_width,
                    format!("{marker}   {}", "  ".repeat(depth)),
                    primary,
                    &agent.title,
                    style,
                    agent_style,
                    selected,
                    app.started.elapsed(),
                    right,
                ));
            }
            Selection::ChildWorktree { repo, agent, child } => {
                let repo = &app.registry.repos[repo];
                let depth = crate::model::agent_depth(&repo.agents, agent);
                let child = &repo.agents[agent].children[child];
                let label = child.branch.as_deref().unwrap_or_else(|| {
                    child
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("worktree")
                });
                let child_style = if selected { style } else { THEME.hint_style() };
                let mut spans = vec![Span::styled(
                    format!("{marker}     {}└ {label}", "  ".repeat(depth)),
                    child_style,
                )];
                if child.clean == Some(false) {
                    spans.push(Span::styled(" *", child_style));
                }
                if child.tmux_session.is_some() {
                    spans.push(Span::styled(" ⌁", child_style));
                }
                lines.push(Line::from(spans));
            }
            Selection::OtherHeader => {
                let count = app.other_location_repos().len();
                let arrow = if app.other_collapsed { "▸" } else { "▾" };
                let noun = if count == 1 { "repo" } else { "repos" };
                let header_style = if selected { style } else { THEME.hint_style() };
                lines.push(Line::from(Span::styled(
                    format!("{marker} {arrow} OTHER LOCATIONS ({count} {noun})"),
                    header_style,
                )));
            }
            Selection::OrphanHeader => {
                let count = app.orphans.len();
                let arrow = if app.orphans_collapsed { "▸" } else { "▾" };
                let noun = if count == 1 { "session" } else { "sessions" };
                let header_style = if selected { style } else { THEME.hint_style() };
                lines.push(Line::from(Span::styled(
                    format!("{marker} {arrow} ORPHAN SESSIONS ({count} {noun})"),
                    header_style,
                )));
            }
            Selection::Orphan(orphan_idx) => {
                let Some(orphan) = app.orphans.get(orphan_idx) else {
                    continue;
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{marker}   "), style),
                    Span::styled(orphan.name.clone(), style),
                    Span::styled(
                        format!("  {}", short_path(&orphan.cwd)),
                        if selected { style } else { THEME.muted_style() },
                    ),
                ]));
            }
        }
    }

    let tree = Paragraph::new(lines)
        .block(
            Block::default()
                .title("PROJECTS")
                .borders(Borders::ALL)
                .title_style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .style(THEME.accent_style());
    frame.render_widget(tree, panes.tree);

    footer::draw(frame, app, root.footer, message);
}

fn short_path(path: &std::path::Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
