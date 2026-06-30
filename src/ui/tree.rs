use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{
    model::{Selection, Status},
    ui::App,
};

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection], message: Option<&str>) {
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

    let header = Paragraph::new("ROBCO ▸ repo-oriented bot control & orchestration")
        .style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, root[0]);

    let mut lines = Vec::new();
    for (idx, item) in visible.iter().enumerate() {
        let selected = idx == app.selected;
        let marker = if selected { ">" } else { " " };
        let style = if selected {
            Style::default().fg(Color::Black).bg(Color::Green)
        } else {
            Style::default().fg(Color::Green)
        };

        match *item {
            Selection::Repo(repo_idx) => {
                let repo = &app.registry.repos[repo_idx];
                let expanded = app.expanded.get(repo_idx).copied().unwrap_or(true);
                let prefix = if expanded { "▾" } else { "▸" };
                let dropr = if repo.dropr.is_some() { " dropr" } else { "" };
                lines.push(Line::from(vec![Span::styled(
                    format!(
                        "{marker} {prefix} {} ({}){dropr}",
                        repo.name,
                        repo.agents.len()
                    ),
                    style,
                )]));
                if expanded && repo.agents.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "    (no agents)",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            Selection::Agent { repo, agent } => {
                let agent = &app.registry.repos[repo].agents[agent];
                let agent_style = if selected {
                    style
                } else if agent.status == Status::BranchOnly {
                    Style::default().fg(Color::DarkGray)
                } else {
                    style
                };
                let status_text = if agent.status == Status::Running {
                    super::spinner::frame(app.frame)
                } else {
                    agent.status.badge()
                };
                let status_style = if selected {
                    style
                } else {
                    super::status_style(agent.status)
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{marker}   "), style),
                    Span::styled(&agent.title, agent_style),
                    Span::raw(" "),
                    Span::styled(status_text, status_style),
                ]));
            }
        }
    }

    let tree = Paragraph::new(lines)
        .block(Block::default().title("projects").borders(Borders::ALL))
        .style(Style::default().fg(Color::Green));
    frame.render_widget(tree, panes[0]);

    let footer_text = message.unwrap_or(
        "↑↓/jk move  pgup/pgdn scroll  tab diff  enter attach  n/N new  a add repo  s push  x kill  ? help  q quit",
    );
    let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, root[2]);
}
