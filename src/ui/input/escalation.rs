//! Approve or dismiss the escalation attached to a selected worker row.

use crossterm::event::KeyCode;

use crate::{locale::t, model::Selection};

use super::super::App;

pub(super) fn handle_normal(app: &mut App, code: KeyCode) -> bool {
    handle_normal_with(app, code, App::approve_inbox)
}

fn handle_normal_with(app: &mut App, code: KeyCode, approve: impl FnOnce(&mut App, usize)) -> bool {
    if !matches!(code, KeyCode::Char('y') | KeyCode::Char('d')) {
        return false;
    }
    let Some(Selection::Agent { repo, agent }) = app.selected_item() else {
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

    let escalation = match code {
        KeyCode::Char('y') => app
            .escalations_for_agent(&agent_id)
            .into_iter()
            .find(|(_, item)| item.actionable()),
        KeyCode::Char('d') => app.escalations_for_agent(&agent_id).into_iter().next(),
        _ => unreachable!(),
    }
    .map(|(index, _)| index);

    let Some(index) = escalation else {
        app.show_message(t(app.locale, "no escalation on this worker"));
        return true;
    };
    match code {
        KeyCode::Char('y') => approve(app, index),
        KeyCode::Char('d') => app.dismiss_inbox_item(index),
        _ => unreachable!(),
    }
    true
}

#[cfg(test)]
#[path = "escalation_tests.rs"]
mod tests;
