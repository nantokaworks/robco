//! Approve or dismiss the escalation attached to a selected worker row.

use crossterm::event::KeyCode;

use crate::{locale::t, model::Selection};

use super::super::App;

pub(super) fn handle_normal(app: &mut App, code: KeyCode) -> bool {
    handle_normal_with(app, code, App::approve_inbox)
}

fn handle_normal_with(app: &mut App, code: KeyCode, approve: impl FnOnce(&mut App, usize)) -> bool {
    let selection = app.selected_item();
    if let Some(Selection::OverseerAlert(item) | Selection::RepoEscalation { item, .. }) = selection
    {
        if !matches!(
            code,
            KeyCode::Char('y') | KeyCode::Char('d') | KeyCode::Enter
        ) {
            return false;
        }
        return handle_display_only(app, selection.unwrap(), item, code);
    }
    if !matches!(code, KeyCode::Char('y') | KeyCode::Char('d')) {
        return false;
    }
    let Some(Selection::Agent { repo, agent }) = selection else {
        return false;
    };
    let Some(agent_id) = app
        .registry
        .repos
        .get(repo)
        .and_then(|repo| repo.agents.get(agent))
        .map(|agent| agent.id.clone())
    else {
        return false;
    };

    let approving = code == KeyCode::Char('y');
    let escalation = if approving {
        app.escalations_for_agent(&agent_id)
            .into_iter()
            .find(|(_, item)| item.actionable())
    } else {
        app.escalations_for_agent(&agent_id).into_iter().next()
    }
    .map(|(index, _)| index);

    let Some(index) = escalation else {
        app.show_message(t(app.locale, "no escalation on this worker"));
        return true;
    };
    if approving {
        approve(app, index);
    } else {
        app.dismiss_inbox_item(index);
    }
    true
}

fn handle_display_only(app: &mut App, selection: Selection, index: usize, code: KeyCode) -> bool {
    handle_display_only_with(app, selection, index, code, App::dismiss_inbox_item)
}

fn handle_display_only_with(
    app: &mut App,
    selection: Selection,
    index: usize,
    code: KeyCode,
    dismiss: impl FnOnce(&mut App, usize),
) -> bool {
    let listed = match selection {
        Selection::OverseerAlert(_) => app
            .overseer_inbox
            .get(index)
            .is_some_and(|item| item.repo.is_none()),
        Selection::RepoEscalation { repo, .. } => {
            app.registry.repos.get(repo).is_some_and(|repo| {
                app.escalations_for_repo(&repo.name)
                    .iter()
                    .any(|(i, _)| *i == index)
            })
        }
        _ => false,
    };
    if !listed {
        app.show_message(t(app.locale, "inbox item is no longer listed"));
    } else if code == KeyCode::Char('d') {
        dismiss(app, index);
    } else {
        app.show_message(t(app.locale, super::inbox_respond::DISPLAY_ONLY));
    }
    true
}

#[cfg(test)]
#[path = "escalation_tests.rs"]
mod tests;
