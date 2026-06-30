use ansi_to_tui::IntoText;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{
    git,
    model::{Selection, Status},
    registry::Registry,
    tmux,
    ui::PreviewPane,
};

pub fn draw(
    frame: &mut Frame<'_>,
    selection: Option<Selection>,
    registry: &Registry,
    pane: PreviewPane,
    scroll: u16,
) {
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

    let (title, text) = match (pane, selection) {
        (PreviewPane::Help, _) => help(),
        (_, Some(Selection::Repo(repo_idx))) => repo_summary(&registry.repos[repo_idx]),
        (PreviewPane::Terminal, Some(Selection::Agent { repo, agent })) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("preview: {} / {}", repo.name, agent.title);
            if agent.status == Status::BranchOnly {
                return render_branch_only(frame, panes[1], title, &agent.branch);
            }
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
        (PreviewPane::Diff, Some(Selection::Agent { repo, agent })) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("diff: {} / {}", repo.name, agent.title);
            if agent.status == Status::BranchOnly {
                return render_branch_only(frame, panes[1], title, &agent.branch);
            }
            let text = git::diff(&agent.worktree_path)
                .unwrap_or_else(|err| err.to_string())
                .into_text()
                .unwrap_or_else(|_| vec![Line::from("Could not render diff.")].into());
            (title, text)
        }
        (_, None) => (
            "preview".to_string(),
            vec![Line::from("No repositories discovered.")].into(),
        ),
    };

    let preview = Paragraph::new(text)
        .block(Block::default().title(title).borders(Borders::ALL))
        .style(Style::default().fg(Color::Green))
        .scroll((scroll, 0));
    frame.render_widget(preview, panes[1]);
}

fn render_branch_only(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    title: String,
    branch: &str,
) {
    let text = vec![
        Line::from(Span::styled(
            "Worktree has been removed.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(vec![
            Span::styled("branch: ", Style::default().fg(Color::DarkGray)),
            Span::raw(branch.to_string()),
        ]),
        Line::from(Span::styled(
            "Press x to delete the branch.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let preview = Paragraph::new(text)
        .block(Block::default().title(title).borders(Borders::ALL))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(preview, area);
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

fn help() -> (String, ratatui::text::Text<'static>) {
    (
        "help".to_string(),
        vec![
            Line::from("Navigation"),
            Line::from("  j/k or arrows  move selection"),
            Line::from("  h/l            collapse or expand repo"),
            Line::from("  tab            switch terminal/diff view"),
            Line::from(""),
            Line::from("Sessions"),
            Line::from("  n              new agent under selected repo"),
            Line::from("  N              new agent with initial prompt: title | prompt"),
            Line::from("  enter          attach to selected tmux session"),
            Line::from("  t              open a shell in the agent worktree (ctrl-q to return)"),
            Line::from("  ctrl-q         return from attached tmux session"),
            Line::from("  r              restart selected agent"),
            Line::from(
                "  x              remove selected agent worktree, then optionally delete branch",
            ),
            Line::from(""),
            Line::from("Repo / shipping"),
            Line::from("  a              add repository by path"),
            Line::from("  s              git add, commit, and push selected agent branch"),
            Line::from(""),
            Line::from("General"),
            Line::from("  ?              show this help"),
            Line::from("  q              quit without stopping agents"),
        ]
        .into(),
    )
}
