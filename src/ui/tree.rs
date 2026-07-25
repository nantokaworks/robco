use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::model::{ManagementMode, Selection, Status};
use crate::subagents::SubagentStatus;

use super::{App, layout, theme::DEFAULT as THEME};
use indicator::{IndicatorState, select, select_supplementary};

mod footer;
mod hints;
mod indicator;
mod label;
pub(in crate::ui) mod overseer_frame;

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection], message: Option<&str>) {
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body, app.overseer_frame_height());
    if app.overseer_visible {
        overseer_frame::draw(frame, app, panes.overseer);
    }
    let projects_width = panes.tree.width.saturating_sub(1);

    let mut lines = vec![Line::from(Span::styled(
        "PROJECTS",
        THEME.accent_bold_style(),
    ))];
    for (idx, item) in visible.iter().enumerate() {
        let selected = idx == app.selected;
        let marker = if selected { ">" } else { " " };
        let style = if selected {
            THEME.selection_style()
        } else {
            THEME.accent_style()
        };

        match *item {
            Selection::OverseerCategory(_) | Selection::OverseerInbox(_) => continue,
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
                        format!("    {}(no agents)", label::AGENT_INDENT),
                        THEME.muted_style(),
                    )));
                }
            }
            Selection::Agent {
                repo: repo_idx,
                agent: agent_idx,
            } => {
                let repo = &app.registry.repos[repo_idx];
                let depth = crate::model::agent_depth(&repo.agents, agent_idx);
                let agent = &repo.agents[agent_idx];
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
                indicator_state.mcp_active = agent.mcp_active;
                indicator_state.subagents_active = active;
                let primary = select(indicator_state);
                let right = indicator::supplementary_spans(
                    primary,
                    select_supplementary(indicator_state),
                    selected,
                    " ",
                );
                let has_children = agent
                    .children
                    .iter()
                    .any(|child| super::actions::children::child_is_visible(agent, child));
                let child_marker = has_children.then(|| {
                    if app.agent_children_expanded(repo_idx, agent_idx) {
                        "▾ "
                    } else {
                        "▸ "
                    }
                });
                let overseer_auto =
                    crate::overseer::is_overseer_child(agent.parent_agent_id.as_deref())
                        && agent.management == ManagementMode::Auto;
                let prefix = label::agent_row_prefix(marker, overseer_auto, depth, child_marker);
                lines.push(label::labeled_row(
                    projects_width,
                    prefix,
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
                    format!(
                        "{marker}     {}{}└ {label}",
                        label::AGENT_INDENT,
                        "  ".repeat(depth)
                    ),
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

    let tree = Paragraph::new(lines).style(THEME.accent_style());
    let projects_area = Rect {
        width: projects_width,
        ..panes.tree
    };
    frame.render_widget(tree, projects_area);

    footer::draw(frame, app, root.footer, message);
}

fn short_path(path: &std::path::Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests;
