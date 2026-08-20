use ratatui::{
    layout::Rect,
    text::{Line, Span},
};

use super::super::{App, Mode, error_dialog, help, input_wrap, spinner, theme::DEFAULT as THEME};
use crate::locale::{fmt, t};

#[path = "content_widgets.rs"]
mod content_widgets;
#[path = "inbox_dismiss_content.rs"]
mod inbox_dismiss_content;

use content_widgets::{CLEANUP_FOLLOWS, confirm_lines, hint_line, input_line};

/// A dialog's title and body, plus where the text caret belongs inside it.
pub(super) struct DialogContent {
    pub(super) title: String,
    pub(super) lines: Vec<Line<'static>>,
    /// `(row, column)` of the caret within `lines`, for the modes that edit
    /// text. `None` keeps the terminal caret wherever the caller parked it.
    pub(super) caret: Option<(usize, usize)>,
}

pub(super) fn content(app: &App, body: Rect) -> Option<DialogContent> {
    let locale = app.locale;
    let content_width = body.width.saturating_sub(4) as usize;
    let (title, lines, caret): (&str, Vec<Line<'static>>, Option<(usize, usize)>) = match &app.mode
    {
        Mode::PromptAgent { input, .. } => {
            let (line, column) = input_line(t(locale, "agent"), input);
            let lines = vec![
                Line::from(Span::styled(
                    t(
                        locale,
                        "Create a new agent with an optional | initial prompt.",
                    ),
                    THEME.accent_style(),
                )),
                Line::from(""),
                line,
                hint_line(locale, "format: title | initial prompt"),
                Line::from(""),
                hint_line(locale, "enter create   esc cancel"),
            ];
            (t(locale, "new agent"), lines, Some((2, column)))
        }
        Mode::PromptRepo { input } => {
            let (line, column) = input_line(t(locale, "git URL or path"), input);
            (
                t(locale, "add repo"),
                vec![
                    line,
                    hint_line(locale, "git format: <git-url> [branch]"),
                    hint_line(locale, "enter add   esc cancel"),
                ],
                Some((0, column)),
            )
        }
        Mode::PromptOverseer { input } => {
            let max_input_height = body.height.saturating_sub(4).clamp(1, 10) as usize;
            let wrapped = input_wrap::input_lines(
                t(locale, "instruction"),
                input,
                content_width,
                max_input_height,
            );
            let caret = wrapped.caret;
            let mut lines = wrapped.lines;
            lines.push(hint_line(locale, "enter send   esc cancel"));
            (t(locale, "instruct overseer control"), lines, Some(caret))
        }
        Mode::PromptInbox { item, input } => {
            let max_input_height = body.height.saturating_sub(5).clamp(1, 10) as usize;
            let mut lines = vec![Line::from(fmt(locale, "target: {}", &[&item.label]))];
            let wrapped = input_wrap::input_lines(
                t(locale, "answer"),
                input,
                content_width,
                max_input_height,
            );
            let caret = (wrapped.caret.0 + lines.len(), wrapped.caret.1);
            lines.extend(wrapped.lines);
            lines.push(hint_line(locale, "enter send   esc cancel"));
            (t(locale, "answer overseer inbox"), lines, Some(caret))
        }
        Mode::ConfirmKill { repo, agent } => (
            t(locale, "delete worktree?"),
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].title.clone()),
                Line::from(t(
                    locale,
                    "ends the running session now and removes the worktree",
                )),
                hint_line(locale, "y delete   n/esc cancel"),
            ],
            None,
        ),
        Mode::ConfirmRemoveRepo { path } => (
            t(locale, "remove repo?"),
            confirm_lines(
                locale,
                path.display().to_string(),
                "y remove   n/esc cancel",
            ),
            None,
        ),
        Mode::ConfirmMerge {
            repo, agent, plan, ..
        } => {
            let mut lines = vec![Line::from(
                app.registry.repos[*repo].agents[*agent].branch.clone(),
            )];
            if *plan == super::super::LandPlan::MergeNow {
                lines.push(Line::from(fmt(
                    locale,
                    "strategy: {}",
                    &[app.config.merge_strategy.label()],
                )));
            }
            lines.push(Line::from(match plan {
                super::super::LandPlan::MergeNow => t(locale, "It will merge now"),
                super::super::LandPlan::QueueApproval => {
                    t(locale, "Approval is queued; it will merge once the checks pass")
                }
                super::super::LandPlan::OpenPrThenQueue => t(
                    locale,
                    "It will open a pull request and queue approval; it will merge once the checks pass",
                ),
            }));
            if *plan == super::super::LandPlan::MergeNow {
                lines.push(Line::from(t(locale, CLEANUP_FOLLOWS)));
            }
            lines.push(hint_line(locale, "y land   n/esc cancel"));
            (t(locale, "land task?"), lines, None)
        }
        Mode::ConfirmCleanup { repo, agent } => (
            t(locale, "clean up merged PR?"),
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                Line::from(t(locale, "already merged")),
                Line::from(t(locale, CLEANUP_FOLLOWS)),
                hint_line(locale, "y clean up   n/esc cancel"),
            ],
            None,
        ),
        Mode::PrPrecheck { branch, .. } => {
            let lines = vec![
                Line::from(fmt(locale, "branch: {}", &[branch])),
                Line::from(Span::styled(
                    fmt(
                        locale,
                        "checking session/PR… {}",
                        &[spinner::frame(app.started.elapsed())],
                    ),
                    THEME.accent_style(),
                )),
                hint_line(locale, "esc cancel"),
            ];
            (t(locale, "request PR from agent?"), lines, None)
        }
        Mode::ConfirmPr { branch, input, .. } => {
            let max_input_height = body.height.saturating_sub(4).clamp(1, 10) as usize;
            let mut lines = vec![Line::from(fmt(locale, "branch: {}", &[branch]))];
            let wrapped = input_wrap::input_lines(
                t(locale, "prompt"),
                input,
                content_width,
                max_input_height,
            );
            let caret = (wrapped.caret.0 + lines.len(), wrapped.caret.1);
            lines.extend(wrapped.lines);
            lines.push(hint_line(
                locale,
                "enter send   ctrl-s save only   esc cancel",
            ));
            (t(locale, "request PR from agent?"), lines, Some(caret))
        }
        Mode::ConfirmDeleteBranch { repo, agent } => (
            t(locale, "delete branch?"),
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                Line::from(t(
                    locale,
                    "force delete: any commits not merged elsewhere are lost",
                )),
                hint_line(locale, "y delete   n/esc keep"),
            ],
            None,
        ),
        Mode::ConfirmKillOrphan { session } => (
            t(locale, "kill session?"),
            confirm_lines(locale, session.clone(), "y kill   n/esc cancel"),
            None,
        ),
        Mode::ConfirmOverseerPanic => (
            t(locale, "stop overseer?"),
            vec![
                Line::from(t(locale, "kill all overseer workers")),
                Line::from(t(locale, "daemon stays alive")),
                hint_line(locale, "y stop   n/esc cancel"),
            ],
            None,
        ),
        Mode::ConfirmDaemonStop => (
            t(locale, "stop the overseer daemon?"),
            vec![
                Line::from(t(
                    locale,
                    "ends the daemon process itself, not just dispatch",
                )),
                Line::from(t(
                    locale,
                    "running workers are not touched; start it again with R",
                )),
                hint_line(locale, "y stop   n/esc cancel"),
            ],
            None,
        ),
        Mode::ConfirmInboxDismissAll { count } => (
            t(locale, "clear the overseer inbox?"),
            inbox_dismiss_content::body(locale, *count, &app.overseer_inbox),
            None,
        ),
        Mode::ConfirmRemoveDiscordChannel { label, .. } => (
            t(locale, "remove channel?"),
            vec![
                Line::from(label.clone()),
                Line::from(t(
                    locale,
                    "deletes its whole record, history included — this cannot be undone",
                )),
                hint_line(locale, "y remove   n/esc cancel"),
            ],
            None,
        ),
        Mode::ErrorDialog {
            title,
            lines,
            force_kill,
        } => (
            title,
            error_dialog::content(locale, lines, force_kill.is_some()),
            None,
        ),
        Mode::Help { .. } => (t(locale, "help"), help::lines(locale), None),
        // Drawn by `super::draw`'s own `task_body` branch, ahead of this
        // function, since its content is per-task and dynamic rather than
        // the static translated strings every arm above builds from.
        Mode::TaskBody { .. } | Mode::Normal => return None,
    };

    Some(DialogContent {
        title: title.to_string(),
        lines,
        caret,
    })
}

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;
