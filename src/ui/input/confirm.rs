use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};

use crate::{Result, locale::t, model::ManagementMode};

use super::{App, Mode, management};

pub(super) fn handle_confirm(app: &mut App, key: KeyEvent) -> Option<Result<()>> {
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
            | Mode::ConfirmOverseerReset
            | Mode::ConfirmInboxDismissAll { .. }
            | Mode::ConfirmOverseerBulkToggle { .. } => Some(Ok(())),
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
        Mode::ConfirmMerge { repo, agent } => {
            let (repo, agent) = (*repo, *agent);
            if confirmed {
                app.start_merge(repo, agent);
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
        Mode::ConfirmOverseerReset => {
            app.mode = Mode::Normal;
            if confirmed {
                app.reset_overseer();
            }
            Some(Ok(()))
        }
        Mode::ConfirmInboxDismissAll { .. } => {
            app.mode = Mode::Normal;
            if confirmed {
                app.dismiss_inbox_all();
            }
            Some(Ok(()))
        }
        Mode::ConfirmOverseerBulkToggle {
            repo_path, target, ..
        } => {
            let (repo_path, target): (PathBuf, ManagementMode) = (repo_path.clone(), *target);
            app.mode = Mode::Normal;
            Some(if confirmed {
                management::bulk_toggle_repo(app, &repo_path, target)
            } else {
                Ok(())
            })
        }
        _ => None,
    }
}
