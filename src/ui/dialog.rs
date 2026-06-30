use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::model::Selection;

use super::{App, Mode};

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection]) {
    let (title, lines): (&str, Vec<Line<'static>>) = match &app.mode {
        Mode::PromptAgent {
            with_prompt, input, ..
        } => {
            let label = if *with_prompt {
                "title | initial prompt"
            } else {
                "agent title"
            };
            (
                "new agent",
                vec![
                    input_line(label, input),
                    hint_line("enter create   esc cancel"),
                ],
            )
        }
        Mode::PromptRepo { input } => (
            "add repo",
            vec![
                input_line("repo path", input),
                hint_line("enter add   esc cancel"),
            ],
        ),
        Mode::ConfirmKill { repo, agent } => (
            "delete worktree?",
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].title.clone()),
                hint_line("y delete   n/esc cancel"),
            ],
        ),
        Mode::ConfirmDeleteBranch { repo, agent } => (
            "delete branch?",
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                hint_line("y delete   n/esc keep"),
            ],
        ),
        Mode::Help => (
            "help",
            vec![
                Line::from("Navigation"),
                Line::from("  j/k or arrows  move selection"),
                Line::from("  h/l            collapse or expand repo"),
                Line::from("  tab            cycle claude / diff / terminal view"),
                Line::from(""),
                Line::from("Sessions"),
                Line::from("  n              new agent under selected repo"),
                Line::from("  N              new agent with initial prompt: title | prompt"),
                Line::from("  enter          attach to claude, or shell in terminal view"),
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
                Line::from(""),
                hint_line("press any key to close"),
            ],
        ),
        Mode::Message(_) | Mode::Normal => return,
    };

    let width = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(title.len()) as u16
        + 4;
    let height = lines.len() as u16 + 2;
    let area = if matches!(&app.mode, Mode::Help) {
        centered_area(frame, width, height)
    } else {
        popup_area(frame, app, visible, width, height)
    };

    let block = Block::default()
        .title(title)
        .title_style(Style::default().add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .style(Style::default().bg(Color::Black));
    let dialog = Paragraph::new(lines)
        .block(block)
        .style(Style::default().fg(Color::Green));
    frame.render_widget(Clear, area);
    frame.render_widget(dialog, area);
}

fn input_line(label: &str, input: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {label}: "),
            Style::default().fg(Color::Black).bg(Color::Green),
        ),
        Span::styled(input.to_string(), Style::default().fg(Color::White)),
        Span::styled("_", Style::default().fg(Color::Green)),
    ])
}

fn hint_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(Color::Gray),
    ))
}

fn centered_area(frame: &Frame<'_>, width: u16, height: u16) -> Rect {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let container = root[1];

    let width = width.min(container.width);
    let height = height.min(container.height);
    let x = container.x + container.width.saturating_sub(width) / 2;
    let y = container.y + container.height.saturating_sub(height) / 2;

    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Place the dialog just below the selected tree row, clamped inside the
/// content pane. Falls back to above the row when there is no room below.
fn popup_area(
    frame: &Frame<'_>,
    app: &App,
    visible: &[Selection],
    width: u16,
    height: u16,
) -> Rect {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(root[1]);
    let container = root[1];
    let tree = panes[0];

    let width = width.min(container.width);
    let height = height.min(container.height);

    // The tree block reserves its top row for the "PROJECTS" title.
    let anchor_row = tree.y + 1 + selected_row_offset(app, visible);

    let x = tree.x.min(container.right().saturating_sub(width));
    let below = anchor_row.saturating_add(1);
    let y = if below + height <= container.bottom() {
        below
    } else {
        anchor_row.saturating_sub(height)
    };
    let y = y
        .max(container.y)
        .min(container.bottom().saturating_sub(height));

    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Number of rendered rows above the selected item, accounting for the extra
/// "(no agents)" line drawn under an expanded empty repo.
fn selected_row_offset(app: &App, visible: &[Selection]) -> u16 {
    let mut offset = 0u16;
    for (idx, item) in visible.iter().enumerate() {
        if idx == app.selected {
            break;
        }
        offset += 1;
        if let Selection::Repo(repo_idx) = item {
            let expanded = app.expanded.get(*repo_idx).copied().unwrap_or(true);
            if expanded && app.registry.repos[*repo_idx].agents.is_empty() {
                offset += 1;
            }
        }
    }
    offset
}
