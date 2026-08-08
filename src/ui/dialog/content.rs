use ratatui::{
    layout::Rect,
    text::{Line, Span},
};

use super::super::{
    App, Mode, error_dialog, help, input::management, input_wrap, spinner, text_input::TextInput,
    theme::DEFAULT as THEME,
};
use crate::locale::{Locale, fmt, t};

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
            confirm_lines(
                locale,
                app.registry.repos[*repo].agents[*agent].title.clone(),
                "y delete   n/esc cancel",
            ),
            None,
        ),
        Mode::ConfirmOverseerBulkToggle {
            repo_name,
            target,
            count,
            ..
        } => (
            t(locale, "manage whole repo?"),
            vec![
                Line::from(repo_name.clone()),
                Line::from(fmt(
                    locale,
                    "{} worker(s) {}",
                    &[&count.to_string(), management::bulk_action(*target)],
                )),
                hint_line(locale, "y apply   n/esc cancel"),
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
        Mode::ConfirmMerge { repo, agent } => (
            t(locale, "merge?"),
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                Line::from(fmt(
                    locale,
                    "strategy: {}",
                    &[app.config.merge_strategy.label()],
                )),
                hint_line(locale, "y merge   n/esc cancel"),
            ],
            None,
        ),
        Mode::ConfirmCleanup { repo, agent } => (
            t(locale, "clean up merged PR?"),
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                Line::from(t(
                    locale,
                    "already merged: pull main, remove worktree, delete branch",
                )),
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
            confirm_lines(
                locale,
                app.registry.repos[*repo].agents[*agent].branch.clone(),
                "y delete   n/esc keep",
            ),
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
                Line::from(t(locale, "disable dispatch + kill all overseer workers")),
                Line::from(t(
                    locale,
                    "daemon stays alive; press S again to turn dispatch back on",
                )),
                hint_line(locale, "y stop   n/esc cancel"),
            ],
            None,
        ),
        Mode::ConfirmOverseerReset => (
            t(locale, "reset dispatch circuit?"),
            vec![
                Line::from(t(
                    locale,
                    "re-enable dispatch and clear the failure counter",
                )),
                hint_line(locale, "y reset   n/esc cancel"),
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
            vec![
                Line::from(fmt(
                    locale,
                    "hide all {} listed item(s)",
                    &[&count.to_string()],
                )),
                Line::from(t(
                    locale,
                    "decisions.jsonl and ledger.json are not modified;",
                )),
                Line::from(t(
                    locale,
                    "a newer escalation for the same target is listed again",
                )),
                hint_line(locale, "y clear   n/esc cancel"),
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
        Mode::Normal => return None,
    };

    Some(DialogContent {
        title: title.to_string(),
        lines,
        caret,
    })
}

fn confirm_lines(locale: Locale, subject: String, hint: &'static str) -> Vec<Line<'static>> {
    vec![Line::from(subject), hint_line(locale, hint)]
}

/// One-line labelled input, paired with the display column its caret sits at.
fn input_line(label: &str, input: &TextInput) -> (Line<'static>, usize) {
    let prefix = format!(" {label}: ");
    let column =
        input_wrap::display_width(&prefix) + input_wrap::text_width(input.text(), input.cursor());
    let mut spans = vec![Span::styled(prefix, THEME.dialog_label_style())];
    spans.extend(input_wrap::input_spans(input.text(), Some(input.cursor())));
    (Line::from(spans), column)
}

fn hint_line(locale: Locale, text: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        t(locale, text).to_string(),
        THEME.hint_style(),
    ))
}
