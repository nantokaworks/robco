use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::model::{Selection, Status};

use super::{App, layout, theme::DEFAULT as THEME};

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection], message: Option<&str>) {
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body);

    let header = Paragraph::new("ROBCO ▸ repo-oriented bot control & orchestration")
        .style(THEME.accent_bold_style());
    frame.render_widget(header, root.header);

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
                let worktree_label = if repo.agents.len() == 1 {
                    "worktree"
                } else {
                    "worktrees"
                };
                let mut spans = vec![Span::styled(
                    format!(
                        "{marker} {prefix} {} ({} {worktree_label})",
                        repo.name,
                        repo.agents.len()
                    ),
                    style,
                )];
                // The repo's own main-worktree AI session progress. Shown only
                // when such a session is running, so the parent node reflects AI
                // work done directly on `main`.
                if let Some(status) = repo.main_status {
                    let status_text = if status == Status::Running {
                        super::spinner::frame(app.started.elapsed())
                    } else {
                        status.badge()
                    };
                    let status_style = if selected {
                        THEME.selected_status_style(status)
                    } else {
                        super::status_style(status)
                    };
                    spans.push(Span::styled("  ", style));
                    spans.push(Span::styled(status_text, status_style));
                }
                if repo.main_shell_working {
                    spans.push(Span::styled(" ", style));
                    spans.push(Span::styled(
                        super::spinner::term_frame(app.started.elapsed()),
                        THEME.term_style(),
                    ));
                }
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
                            super::status_style(status)
                        }
                    };
                    let mut first = true;
                    for (status, count) in status_counts {
                        if count == 0 {
                            continue;
                        }
                        spans.push(Span::styled(if first { "  " } else { " · " }, style));
                        spans.push(Span::styled(
                            format!("{count} {}", status.badge()),
                            status_style(status),
                        ));
                        first = false;
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
                let agent = &app.registry.repos[repo].agents[agent];
                let agent_style = if selected {
                    style
                } else if agent.status == Status::BranchOnly {
                    THEME.status_style(Status::BranchOnly)
                } else {
                    style
                };
                let status_text = if agent.status == Status::Running {
                    super::spinner::frame(app.started.elapsed())
                } else {
                    agent.status.badge()
                };
                let status_style = if selected {
                    THEME.selected_status_style(agent.status)
                } else {
                    super::status_style(agent.status)
                };
                let mut spans = vec![
                    Span::styled(format!("{marker}   "), style),
                    Span::styled(&agent.title, agent_style),
                    Span::raw(" "),
                    Span::styled(status_text, status_style),
                ];
                if agent.shell_working {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        super::spinner::term_frame(app.started.elapsed()),
                        THEME.term_style(),
                    ));
                }
                lines.push(Line::from(spans));
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

    let footer_text = message.unwrap_or(
        "↑↓/jk move  pgup/pgdn scroll  tab diff  enter attach  t shell  n/N new  a add repo  s push  x kill  ? help  q quit",
    );
    let footer = Paragraph::new(footer_text).style(THEME.muted_style());
    frame.render_widget(footer, root.footer);
}
