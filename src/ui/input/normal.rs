//! Normal-mode key table, split out so `ui::input` remains a thin mode router.
//!
//! Guard order is load-bearing: the first sub-router that accepts a key wins.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    Result,
    locale::{fmt, t},
    model::Selection,
    ui::{App, Mode, PreviewPane, text_input::TextInput},
};

use super::{dropr_task_drill, escalation, host_connect, overseer};

pub(super) fn handle(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        // Keep this chain in exact dispatch order: the first handler wins.
        code if host_connect::handle_normal(app, code) => {}
        code if overseer::handle_normal(app, code) => {}
        code if escalation::handle_normal(app, code) => {}
        code if dropr_task_drill::handle_normal(app, code) => {}
        KeyCode::Char('q') | KeyCode::Esc => {
            let merging = app.merging_branches();
            let launching = app.launching_tasks();
            if !merging.is_empty() {
                app.show_message(fmt(
                    app.locale,
                    "merge in progress: {} — wait or ctrl-c to force quit",
                    &[&merging.join(", ")],
                ));
            } else if !launching.is_empty() {
                app.show_message(fmt(
                    app.locale,
                    "launch in progress: {} — wait or ctrl-c to force quit",
                    &[&launching.join(", ")],
                ));
            } else if matches!(key.code, KeyCode::Esc) && app.dismiss_merge_outcome() {
                app.show_message(t(app.locale, "dismissed merge notice"));
            } else {
                return Ok(true);
            }
        }
        // Matched ahead of the plain arrow arms below: those dispatch on
        // `key.code` alone, so without this a shift-modified arrow would
        // fall into them and move the cursor instead of the row.
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.move_selected_repo(-1);
        }
        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.move_selected_repo(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_selection_down();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_selection_up();
        }
        KeyCode::PageDown => app.scroll_preview(false, 10),
        KeyCode::PageUp => app.scroll_preview(true, 10),
        KeyCode::Right | KeyCode::Char('l') => app.expand_selected_tree_item(),
        KeyCode::Left | KeyCode::Char('h') => app.collapse_selected_tree_item(),
        KeyCode::Char('n') => {
            if let Some(repo) = app.selected_repo() {
                app.mode = Mode::PromptAgent {
                    repo,
                    input: TextInput::new(),
                };
            } else {
                app.mode = Mode::PromptRepo {
                    input: TextInput::new(),
                };
            }
        }
        KeyCode::Char('a') => {
            app.mode = Mode::PromptRepo {
                input: TextInput::new(),
            };
        }
        KeyCode::Tab => app.toggle_preview(),
        KeyCode::BackTab => app.toggle_preview_back(),
        KeyCode::Char('?') => app.mode = Mode::Help { scroll: 0 },
        KeyCode::Enter => match app.selected_item() {
            Some(selection) if app.toggle_selected_tree_header(selection) => {}
            Some(Selection::OverseerAi) => app.attach_control_selected(),
            Some(Selection::DiscordChannel(index)) => {
                app.attach_discord_channel_selected(index);
            }
            Some(
                selection
                @ (Selection::RemoteControlAi(_) | Selection::RemoteDiscordChannel { .. }),
            ) => {
                overseer::attach_remote_chat(app, selection);
            }
            Some(Selection::RemoteHostError(_)) => {}
            // The drill-down's entry point (dropr:475): INFO is the
            // only tab that can show the task list, so this leaves
            // every other tab's `enter` (Claude/Terminal attach)
            // untouched.
            Some(Selection::Repo(_)) if app.preview == PreviewPane::Info => {
                app.enter_dropr_task_list();
            }
            Some(Selection::Orphan(_)) => app.attach_orphan_selected(),
            _ => match app.preview {
                PreviewPane::Terminal => app.attach_shell_selected()?,
                PreviewPane::Claude => app.attach_claude_selected()?,
                _ => app.attach_selected()?,
            },
        },
        KeyCode::Char('r') => app.restart_selected()?,
        KeyCode::Char('m') => app.merge_selected(),
        KeyCode::Char('u') => app.update_branch_selected(),
        KeyCode::Char('c') => app.checkout_main_selected(),
        KeyCode::Char('C') => app.clear_chat_selected(),
        // Only the Claude tab has a live session to type into (see
        // `panes_for`: it is offered only for Repo/Agent/Orphan rows),
        // so gating on the tab rather than the selection type covers
        // exactly those rows without naming them here.
        KeyCode::Char('i') if app.preview == PreviewPane::Claude => {
            match crate::ui::scrollback::live_session(app) {
                Some(session) => {
                    app.mode = Mode::PromptSession {
                        session,
                        host: None,
                        input: TextInput::new(),
                    };
                }
                None => {
                    app.show_message(t(app.locale, "no live session for this tab"));
                }
            }
        }
        KeyCode::Char('p') => app.confirm_pr_selected(),
        KeyCode::Char('x') => app.confirm_kill_selected(),
        KeyCode::Char('g') => app.open_rename_prompt(),
        KeyCode::Char(',') => app.open_settings_editor(),
        KeyCode::Char(ch) if !ch.is_ascii() => {
            app.show_message(t(app.locale, "IME is on; switch to ASCII input"));
        }
        _ => {}
    }

    app.clamp_selection();
    Ok(false)
}
