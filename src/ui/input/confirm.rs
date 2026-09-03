//! Confirm-mode key arms, split out so `ui::input` only routes mode families.

use crossterm::event::{KeyCode, KeyEvent};

use crate::{Result, config::Config, locale::t};

use crate::ui::confirm_pr::{ConfirmPrAction, confirm_pr_action};

use super::{App, Mode};

pub(super) fn handle_confirm(app: &mut App, key: KeyEvent) -> Option<Result<()>> {
    // `y`/`n` still work here on purpose (dropr:551) — dropping them costs
    // muscle memory for nothing gained — but every dialog hint now advertises
    // only enter/esc, so do not "fix" the hint text back to naming y/n.
    let confirmed = matches!(
        key.code,
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter
    );
    let cancelled = matches!(
        key.code,
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc
    );
    if !confirmed && !cancelled {
        return match app.mode {
            Mode::ConfirmKill { .. }
            | Mode::ConfirmRemoveRepo { .. }
            | Mode::ConfirmMerge { .. }
            | Mode::ConfirmCleanup { .. }
            | Mode::ConfirmDeleteBranch { .. }
            | Mode::ConfirmKillOrphan { .. }
            | Mode::ConfirmOverseerPanic
            | Mode::ConfirmDaemonStop
            | Mode::ConfirmRemoveDiscordChannel { .. }
            | Mode::ConfirmClearChat { .. } => Some(Ok(())),
            _ => None,
        };
    }

    match &app.mode {
        Mode::ConfirmKill { repo, agent } => {
            let (repo, agent) = (*repo, *agent);
            app.mode = Mode::Normal;
            Some(if confirmed {
                app.kill_agent(repo, agent)
            } else {
                Ok(())
            })
        }
        Mode::ConfirmRemoveRepo { path } => {
            let path = path.clone();
            app.mode = Mode::Normal;
            Some(if confirmed {
                app.remove_pinned_repo(&path)
            } else {
                Ok(())
            })
        }
        Mode::ConfirmMerge {
            repo,
            agent,
            plan,
            head,
        } => {
            let (repo, agent, plan, head) = (*repo, *agent, *plan, head.clone());
            if confirmed {
                app.confirm_land(repo, agent, plan, head);
            } else {
                app.mode = Mode::Normal;
            }
            Some(Ok(()))
        }
        Mode::ConfirmCleanup { repo, agent } => {
            let (repo, agent) = (*repo, *agent);
            if confirmed {
                app.start_cleanup(repo, agent);
            } else {
                app.mode = Mode::Normal;
            }
            Some(Ok(()))
        }
        Mode::ConfirmDeleteBranch { repo, agent } => {
            let (repo, agent) = (*repo, *agent);
            app.mode = Mode::Normal;
            if confirmed {
                Some(app.delete_agent_branch(repo, agent))
            } else {
                app.show_message(t(app.locale, "kept branch"));
                Some(Ok(()))
            }
        }
        Mode::ConfirmKillOrphan { session } => {
            let session = session.clone();
            app.mode = Mode::Normal;
            if confirmed {
                app.kill_orphan(&session);
            }
            Some(Ok(()))
        }
        Mode::ConfirmOverseerPanic => {
            app.mode = Mode::Normal;
            if confirmed {
                app.panic_overseer();
            }
            Some(Ok(()))
        }
        Mode::ConfirmDaemonStop => {
            app.mode = Mode::Normal;
            if confirmed {
                app.stop_daemon();
            }
            Some(Ok(()))
        }
        Mode::ConfirmRemoveDiscordChannel { channel_id, label } => {
            let (channel_id, label) = (channel_id.clone(), label.clone());
            app.mode = Mode::Normal;
            if confirmed {
                app.remove_discord_channel(&channel_id, &label);
            }
            Some(Ok(()))
        }
        Mode::ConfirmClearChat { path } => {
            let path = path.clone();
            app.mode = Mode::Normal;
            if confirmed {
                app.clear_chat_confirmed(&path);
            }
            Some(Ok(()))
        }
        _ => None,
    }
}

pub(super) fn handle_confirm_pr(
    app: &mut App,
    key: KeyEvent,
    send: impl FnOnce(&str, &str) -> Result<()>,
) -> Result<()> {
    let Mode::ConfirmPr {
        repo_path,
        agent_id,
        input,
        approval_head,
        ..
    } = &mut app.mode
    else {
        unreachable!("checked by confirm router")
    };
    let action = confirm_pr_action(&mut app.config, input, key, Config::save);
    match action {
        ConfirmPrAction::Stay => {}
        ConfirmPrAction::Cancel => app.mode = Mode::Normal,
        ConfirmPrAction::Submit(prompt) => {
            let repo_path = repo_path.clone();
            let agent_id = agent_id.clone();
            let approval_head = approval_head.clone();
            app.mode = Mode::Normal;
            app.request_pr(&repo_path, &agent_id, &prompt, approval_head, send)?;
        }
        ConfirmPrAction::Saved(result) => match result {
            Ok(()) => app.show_message(t(app.locale, "saved PR prompt to config")),
            Err(err) => app.show_message(err.to_string()),
        },
    }
    Ok(())
}
