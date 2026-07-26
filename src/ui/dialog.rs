use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::model::Selection;

use super::{
    App, Mode, error_dialog, help, input::management, input_wrap, layout, spinner,
    theme::DEFAULT as THEME,
};

mod caret;
#[cfg(test)]
mod tests;

use caret::caret_position;

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection]) -> Option<(u16, u16)> {
    let body = layout::root(frame.area()).body;
    let content_width = body.width.saturating_sub(4) as usize;
    let (title, lines): (&str, Vec<Line<'static>>) = match &app.mode {
        Mode::PromptAgent { input, .. } => {
            let lines = vec![
                Line::from(Span::styled(
                    "Create a new agent with an optional | initial prompt.",
                    THEME.accent_style(),
                )),
                Line::from(""),
                input_line("agent", input),
                hint_line("format: title | initial prompt"),
                Line::from(""),
                hint_line("enter create   esc cancel"),
            ];
            ("new agent", lines)
        }
        Mode::PromptRepo { input } => (
            "add repo",
            vec![
                input_line("git URL or path", input),
                hint_line("git format: <git-url> [branch]"),
                hint_line("enter add   esc cancel"),
            ],
        ),
        Mode::PromptOverseer { input } => {
            let max_input_height = body.height.saturating_sub(4).clamp(1, 10) as usize;
            let mut lines =
                input_wrap::input_lines("instruction", input, content_width, max_input_height);
            lines.push(hint_line("enter send   esc cancel"));
            ("instruct overseer control", lines)
        }
        Mode::PromptInbox { label, input, .. } => {
            let max_input_height = body.height.saturating_sub(5).clamp(1, 10) as usize;
            let mut lines = vec![Line::from(format!("target: {label}"))];
            lines.extend(input_wrap::input_lines(
                "answer",
                input,
                content_width,
                max_input_height,
            ));
            lines.push(hint_line("enter send   esc cancel"));
            ("answer overseer inbox", lines)
        }
        Mode::ConfirmKill { repo, agent } => (
            "delete worktree?",
            confirm_lines(
                app.registry.repos[*repo].agents[*agent].title.clone(),
                "y delete   n/esc cancel",
            ),
        ),
        Mode::ConfirmOverseerBulkToggle {
            repo_name,
            target,
            count,
            ..
        } => (
            "manage whole repo?",
            vec![
                Line::from(repo_name.clone()),
                Line::from(format!(
                    "{count} worker{} {}",
                    if *count == 1 { "" } else { "s" },
                    management::bulk_action(*target)
                )),
                hint_line("y apply   n/esc cancel"),
            ],
        ),
        Mode::ConfirmRemoveRepo { path } => (
            "remove repo?",
            confirm_lines(path.display().to_string(), "y remove   n/esc cancel"),
        ),
        Mode::ConfirmMerge { repo, agent } => (
            "merge?",
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                Line::from(format!("strategy: {}", app.config.merge_strategy.label())),
                hint_line("y merge   n/esc cancel"),
            ],
        ),
        Mode::ConfirmCleanup { repo, agent } => (
            "clean up merged PR?",
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                Line::from("already merged: pull main, remove worktree, delete branch"),
                hint_line("y clean up   n/esc cancel"),
            ],
        ),
        Mode::ConfirmPr {
            repo_path,
            agent_id,
            branch,
            input,
        } => {
            let checking = app.pr_precheck_active_for(repo_path, agent_id);
            let max_input_height = body.height.saturating_sub(4).clamp(1, 10) as usize;
            let mut lines = vec![Line::from(format!("branch: {branch}"))];
            if checking {
                lines.push(Line::from(Span::styled(
                    format!(
                        "checking session/PR… {}",
                        spinner::frame(app.started.elapsed())
                    ),
                    THEME.accent_style(),
                )));
            }
            lines.extend(input_wrap::input_lines(
                "prompt",
                input,
                content_width,
                max_input_height,
            ));
            lines.push(hint_line(if checking {
                "esc cancel"
            } else {
                "enter send   ctrl-s save only   esc cancel"
            }));
            ("request PR from agent?", lines)
        }
        Mode::ConfirmDeleteBranch { repo, agent } => (
            "delete branch?",
            confirm_lines(
                app.registry.repos[*repo].agents[*agent].branch.clone(),
                "y delete   n/esc keep",
            ),
        ),
        Mode::ConfirmKillOrphan { session } => (
            "kill session?",
            confirm_lines(session.clone(), "y kill   n/esc cancel"),
        ),
        Mode::ConfirmOverseerPanic => (
            "stop overseer?",
            vec![
                Line::from("disable dispatch + kill all overseer workers"),
                Line::from("daemon stays alive; re-enable with `robco overseer set dispatch on`"),
                hint_line("y stop   n/esc cancel"),
            ],
        ),
        Mode::ConfirmOverseerReset => (
            "reset dispatch circuit?",
            vec![
                Line::from("re-enable dispatch and clear the failure counter"),
                hint_line("y reset   n/esc cancel"),
            ],
        ),
        Mode::ConfirmInboxDismissAll { count } => (
            "clear the overseer inbox?",
            vec![
                Line::from(format!("hide all {count} listed item(s)")),
                Line::from("decisions.jsonl and ledger.json are not modified;"),
                Line::from("a newer escalation for the same target is listed again"),
                hint_line("y clear   n/esc cancel"),
            ],
        ),
        Mode::ErrorDialog {
            title,
            lines,
            force_kill,
        } => (title, error_dialog::content(lines, force_kill.is_some())),
        Mode::Help { .. } => ("help", help::lines()),
        Mode::Normal => return None,
    };

    let cursor_row = match app.mode {
        Mode::PromptAgent { .. } => Some(2),
        Mode::PromptRepo { .. } => Some(0),
        Mode::PromptOverseer { .. } | Mode::PromptInbox { .. } | Mode::ConfirmPr { .. } => {
            Some(lines.len().saturating_sub(2))
        }
        _ => None,
    };

    let width = (lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(title.len()) as u16
        + 4)
    .min(body.width);
    let height = (lines.len() as u16 + 2).min(body.height);
    let area = if matches!(&app.mode, Mode::Help { .. }) {
        layout::centered_area(frame, width, height)
    } else {
        layout::popup_area(frame, app, visible, width, height)
    };

    let (title, scroll) = match app.mode {
        Mode::Help { scroll } => (
            help::scroll_title(scroll, frame.area().height).unwrap_or_else(|| title.to_string()),
            help::clamp_scroll(scroll, frame.area().height),
        ),
        Mode::ConfirmPr { .. } | Mode::PromptOverseer { .. } | Mode::PromptInbox { .. } => {
            let cursor_row = lines.len().saturating_sub(2) as u16;
            let visible_rows = height.saturating_sub(2);
            (
                title.to_string(),
                cursor_row.saturating_add(1).saturating_sub(visible_rows),
            )
        }
        _ => (title.to_string(), 0),
    };
    let cursor = cursor_row.and_then(|row| {
        lines
            .get(row)
            .map(|line| caret_position(area, line, row, scroll))
    });
    let block = Block::default()
        .title(title)
        .title_style(Style::default().add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(THEME.dialog_border_style());
    let dialog = Paragraph::new(lines)
        .scroll((scroll, 0))
        .block(block)
        .style(THEME.accent_style());
    frame.render_widget(Block::default().style(THEME.backdrop_style()), body);

    // `Clear` only resets cells *inside* the popup rect, so a full-width (CJK)
    // glyph in the dimmed background that straddles the popup's left/right border
    // would leave a stray half-cell that corrupts the border. Wiping the whole
    // row-band removes any such glyph; rows above and below stay dimmed.
    let band = Rect {
        x: body.x,
        y: area.y,
        width: body.width,
        height: area.height,
    };
    frame.render_widget(Clear, band);
    // Painting the backdrop under the popup would leave its DIM modifier
    // (`set_style` only adds modifiers), rendering the dialog content dim.
    let right_x = area.x + area.width;
    for side in [
        Rect {
            x: band.x,
            y: band.y,
            width: area.x.saturating_sub(band.x),
            height: band.height,
        },
        Rect {
            x: right_x,
            y: band.y,
            width: (band.x + band.width).saturating_sub(right_x),
            height: band.height,
        },
    ] {
        frame.render_widget(Block::default().style(THEME.backdrop_style()), side);
    }
    frame.render_widget(dialog, area);
    cursor
}

fn confirm_lines(subject: String, hint: &str) -> Vec<Line<'static>> {
    vec![Line::from(subject), hint_line(hint)]
}

fn input_line(label: &str, input: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {label}: "), THEME.dialog_label_style()),
        Span::styled(input.to_string(), THEME.input_style()),
        Span::styled("_", THEME.accent_style()),
    ])
}

fn hint_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), THEME.hint_style()))
}
