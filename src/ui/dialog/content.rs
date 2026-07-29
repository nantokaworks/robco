use ratatui::{
    layout::Rect,
    text::{Line, Span},
};

use super::super::{
    App, Mode, error_dialog, help, input::management, input_wrap, spinner, text_input::TextInput,
    theme::DEFAULT as THEME,
};

/// A dialog's title and body, plus where the text caret belongs inside it.
pub(super) struct DialogContent {
    pub(super) title: String,
    pub(super) lines: Vec<Line<'static>>,
    /// `(row, column)` of the caret within `lines`, for the modes that edit
    /// text. `None` keeps the terminal caret wherever the caller parked it.
    pub(super) caret: Option<(usize, usize)>,
}

pub(super) fn content(app: &App, body: Rect) -> Option<DialogContent> {
    let content_width = body.width.saturating_sub(4) as usize;
    let (title, lines, caret): (&str, Vec<Line<'static>>, Option<(usize, usize)>) = match &app.mode
    {
        Mode::PromptAgent { input, .. } => {
            let (line, column) = input_line("agent", input);
            let lines = vec![
                Line::from(Span::styled(
                    "Create a new agent with an optional | initial prompt.",
                    THEME.accent_style(),
                )),
                Line::from(""),
                line,
                hint_line("format: title | initial prompt"),
                Line::from(""),
                hint_line("enter create   esc cancel"),
            ];
            ("new agent", lines, Some((2, column)))
        }
        Mode::PromptRepo { input } => {
            let (line, column) = input_line("git URL or path", input);
            (
                "add repo",
                vec![
                    line,
                    hint_line("git format: <git-url> [branch]"),
                    hint_line("enter add   esc cancel"),
                ],
                Some((0, column)),
            )
        }
        Mode::PromptOverseer { input } => {
            let max_input_height = body.height.saturating_sub(4).clamp(1, 10) as usize;
            let wrapped =
                input_wrap::input_lines("instruction", input, content_width, max_input_height);
            let caret = wrapped.caret;
            let mut lines = wrapped.lines;
            lines.push(hint_line("enter send   esc cancel"));
            ("instruct overseer control", lines, Some(caret))
        }
        Mode::PromptInbox { label, input, .. } => {
            let max_input_height = body.height.saturating_sub(5).clamp(1, 10) as usize;
            let mut lines = vec![Line::from(format!("target: {label}"))];
            let wrapped = input_wrap::input_lines("answer", input, content_width, max_input_height);
            let caret = (wrapped.caret.0 + lines.len(), wrapped.caret.1);
            lines.extend(wrapped.lines);
            lines.push(hint_line("enter send   esc cancel"));
            ("answer overseer inbox", lines, Some(caret))
        }
        Mode::ConfirmKill { repo, agent } => (
            "delete worktree?",
            confirm_lines(
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
            None,
        ),
        Mode::ConfirmRemoveRepo { path } => (
            "remove repo?",
            confirm_lines(path.display().to_string(), "y remove   n/esc cancel"),
            None,
        ),
        Mode::ConfirmMerge { repo, agent } => (
            "merge?",
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                Line::from(format!("strategy: {}", app.config.merge_strategy.label())),
                hint_line("y merge   n/esc cancel"),
            ],
            None,
        ),
        Mode::ConfirmCleanup { repo, agent } => (
            "clean up merged PR?",
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                Line::from("already merged: pull main, remove worktree, delete branch"),
                hint_line("y clean up   n/esc cancel"),
            ],
            None,
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
            let wrapped = input_wrap::input_lines("prompt", input, content_width, max_input_height);
            let caret = (wrapped.caret.0 + lines.len(), wrapped.caret.1);
            lines.extend(wrapped.lines);
            lines.push(hint_line(if checking {
                "esc cancel"
            } else {
                "enter send   ctrl-s save only   esc cancel"
            }));
            ("request PR from agent?", lines, Some(caret))
        }
        Mode::ConfirmDeleteBranch { repo, agent } => (
            "delete branch?",
            confirm_lines(
                app.registry.repos[*repo].agents[*agent].branch.clone(),
                "y delete   n/esc keep",
            ),
            None,
        ),
        Mode::ConfirmKillOrphan { session } => (
            "kill session?",
            confirm_lines(session.clone(), "y kill   n/esc cancel"),
            None,
        ),
        Mode::ConfirmOverseerPanic => (
            "stop overseer?",
            vec![
                Line::from("disable dispatch + kill all overseer workers"),
                Line::from("daemon stays alive; press S again to turn dispatch back on"),
                hint_line("y stop   n/esc cancel"),
            ],
            None,
        ),
        Mode::ConfirmOverseerReset => (
            "reset dispatch circuit?",
            vec![
                Line::from("re-enable dispatch and clear the failure counter"),
                hint_line("y reset   n/esc cancel"),
            ],
            None,
        ),
        Mode::ConfirmInboxDismissAll { count } => (
            "clear the overseer inbox?",
            vec![
                Line::from(format!("hide all {count} listed item(s)")),
                Line::from("decisions.jsonl and ledger.json are not modified;"),
                Line::from("a newer escalation for the same target is listed again"),
                hint_line("y clear   n/esc cancel"),
            ],
            None,
        ),
        Mode::ErrorDialog {
            title,
            lines,
            force_kill,
        } => (
            title,
            error_dialog::content(lines, force_kill.is_some()),
            None,
        ),
        Mode::Help { .. } => ("help", help::lines(), None),
        Mode::Normal => return None,
    };

    Some(DialogContent {
        title: title.to_string(),
        lines,
        caret,
    })
}

fn confirm_lines(subject: String, hint: &str) -> Vec<Line<'static>> {
    vec![Line::from(subject), hint_line(hint)]
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

fn hint_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), THEME.hint_style()))
}
