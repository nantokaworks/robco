use ansi_to_tui::IntoText;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{model::Selection, registry::Registry, tmux};

pub fn draw(frame: &mut Frame<'_>, selection: Option<Selection>, registry: &Registry) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(root[1]);

    let (title, text) = match selection {
        Some(Selection::Repo(repo_idx)) => repo_summary(&registry.repos[repo_idx]),
        Some(Selection::Agent { repo, agent }) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("preview: {} / {}", repo.name, agent.title);
            let text = tmux::capture_plain(&agent.tmux_session)
                .ok()
                .and_then(|capture| capture.into_text().ok())
                .unwrap_or_else(|| {
                    vec![
                        Line::from(Span::styled(
                            "No preview available.",
                            Style::default().fg(Color::DarkGray),
                        )),
                        Line::from(Span::styled(
                            &agent.tmux_session,
                            Style::default().fg(Color::DarkGray),
                        )),
                    ]
                    .into()
                });
            (title, text)
        }
        None => (
            "preview".to_string(),
            vec![Line::from("No repositories discovered.")].into(),
        ),
    };

    let preview = Paragraph::new(text)
        .block(Block::default().title(title).borders(Borders::ALL))
        .style(Style::default().fg(Color::Green));
    frame.render_widget(preview, panes[1]);
}

fn repo_summary(repo: &crate::model::RepoNode) -> (String, ratatui::text::Text<'static>) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("path: ", Style::default().fg(Color::DarkGray)),
            Span::raw(repo.path.display().to_string()),
        ]),
        Line::from(vec![
            Span::styled("remote: ", Style::default().fg(Color::DarkGray)),
            Span::raw(
                repo.remote_url
                    .clone()
                    .unwrap_or_else(|| "(none)".to_string()),
            ),
        ]),
        Line::from(vec![
            Span::styled("agents: ", Style::default().fg(Color::DarkGray)),
            Span::raw(repo.agents.len().to_string()),
        ]),
    ];

    if let Some(dropr) = &repo.dropr {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "dropr",
            Style::default().fg(Color::Green),
        )));
        lines.push(Line::from(vec![
            Span::styled("kind: ", Style::default().fg(Color::DarkGray)),
            Span::raw(dropr.kind.clone()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("id: ", Style::default().fg(Color::DarkGray)),
            Span::raw(dropr.id.clone()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("name: ", Style::default().fg(Color::DarkGray)),
            Span::raw(dropr.name.clone()),
        ]));
    }

    (format!("repo: {}", repo.name), lines.into())
}
