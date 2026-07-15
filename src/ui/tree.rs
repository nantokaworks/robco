use ratatui::{
    Frame,
    layout::Alignment,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::model::{Selection, Status};
use crate::subagents::SubagentStatus;

use super::{App, layout, theme::DEFAULT as THEME};
use activity::activity_span;
use indicator::{Indicator, IndicatorState, SupplementaryIndicators, select, select_supplementary};

mod activity;
mod hints;
mod indicator;

fn status_style(status: Status) -> Style {
    THEME.status_style(status)
}

fn indicator_spans(
    indicator: Option<Indicator>,
    supplementary: SupplementaryIndicators,
    selected: bool,
    elapsed: std::time::Duration,
    gap: &str,
) -> Vec<Span<'static>> {
    let mut spans = match indicator {
        Some(Indicator::Status(status)) => vec![Span::styled(
            format!("{gap}{}", status.glyph()),
            if selected {
                THEME.selected_status_style(status)
            } else {
                status_style(status)
            },
        )],
        Some(Indicator::Running) => vec![Span::styled(
            format!("{gap}{}", super::spinner::frame(elapsed)),
            if selected {
                THEME.selected_status_style(Status::Running)
            } else {
                status_style(Status::Running)
            },
        )],
        Some(Indicator::Merging) => vec![Span::styled(
            format!("{gap}⇄ {}", super::spinner::frame(elapsed)),
            THEME.hint_style(),
        )],
        Some(Indicator::ShellActivity) => vec![Span::styled(
            format!("{gap}{}", super::spinner::term_frame(elapsed)),
            THEME.term_style(),
        )],
        Some(Indicator::SubagentActivity(active)) => vec![activity_span(active, gap)],
        Some(Indicator::DroprRefresh) => vec![Span::styled(
            format!("{gap}{}", super::spinner::frame(elapsed)),
            THEME.hint_style(),
        )],
        None => Vec::new(),
    };
    if supplementary.worktree_missing {
        let prefix = if spans.is_empty() { gap } else { " " };
        spans.push(Span::styled(
            format!("{prefix}⌦"),
            THEME.worktree_missing_style(selected),
        ));
    }
    if supplementary.merge_failed {
        let prefix = if spans.is_empty() { gap } else { " " };
        spans.push(Span::styled(
            format!("{prefix}merge-failed"),
            THEME.merge_failed_style(selected),
        ));
    }
    spans
}

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection], message: Option<&str>) {
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body);

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
            Selection::Repo(repo_idx) => {
                let repo = &app.registry.repos[repo_idx];
                let expanded = app.expanded.get(repo_idx).copied().unwrap_or(true);
                let prefix = app.config.project_icon.marker(expanded);
                let mut spans = vec![
                    Span::styled(format!("{marker} {prefix} {}", repo.name), style),
                    Span::styled(
                        format!(" {}", repo.agents.len()),
                        if selected { style } else { THEME.hint_style() },
                    ),
                ];
                if !app.repo_is_local(repo) {
                    spans.push(Span::styled(
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
                spans.extend(indicator_spans(
                    select(indicator_state),
                    select_supplementary(indicator_state),
                    selected,
                    app.started.elapsed(),
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
                            status_style(status)
                        }
                    };
                    let mut first = true;
                    for (status, count) in status_counts {
                        if count == 0 {
                            continue;
                        }
                        spans.push(Span::styled(if first { "  " } else { " · " }, style));
                        spans.push(Span::styled(
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
                        spans.push(Span::styled(if first { "  " } else { " · " }, style));
                        spans.push(Span::styled(
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
                        spans.push(Span::styled(if first { "  " } else { " · " }, style));
                        spans.push(Span::styled(
                            format!("{merge_failed_count} merge-failed"),
                            THEME.merge_failed_style(selected),
                        ));
                    }
                }
                lines.push(Line::from(spans));
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
                let mut spans = vec![
                    Span::styled(format!("{marker}   {}", "  ".repeat(depth)), style),
                    Span::styled(&agent.title, agent_style),
                ];
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
                spans.extend(indicator_spans(
                    select(indicator_state),
                    select_supplementary(indicator_state),
                    selected,
                    app.started.elapsed(),
                    " ",
                ));
                lines.push(Line::from(spans));
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
                .title_style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .style(THEME.accent_style());
    frame.render_widget(tree, panes.tree);

    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let ident_width = ("ROBCO ".len() + version.chars().count() + 2) as u16;
    let zones = layout::footer_zones(root.footer, ident_width);

    let ident = Paragraph::new(Line::from(vec![
        Span::styled("ROBCO", THEME.accent_bold_style()),
        Span::styled(format!(" {version}"), THEME.hint_style()),
    ]));
    frame.render_widget(ident, zones.ident);

    let hints = Paragraph::new(hints::hints_line(
        message,
        hints::r_hint_label(app.selected_item()),
    ))
    .alignment(Alignment::Center);
    frame.render_widget(hints, zones.hints);
}

fn short_path(path: &std::path::Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
