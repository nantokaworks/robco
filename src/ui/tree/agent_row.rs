use ratatui::{style::Style, text::Line};

use crate::{
    model::Status,
    subagents::SubagentStatus,
    ui::{App, theme::DEFAULT as THEME},
};

use super::{
    escalation_line,
    indicator::{self, IndicatorState, select, select_supplementary},
    label, reason_line,
};

pub(super) fn build(
    app: &App,
    repo_idx: usize,
    agent_idx: usize,
    selected: bool,
    marker: &str,
    style: Style,
    projects_width: u16,
) -> Vec<Line<'static>> {
    let repo = &app.registry.repos[repo_idx];
    let mut row = crate::model::agent_row(&repo.agents, agent_idx);
    let repo_has_trailing_escalations = repo.host.is_none()
        && app
            .registry
            .repos
            .iter()
            .position(|candidate| candidate.host.is_none() && candidate.name == repo.name)
            == Some(repo_idx)
        && !app.escalations_for_repo(&repo.name).is_empty();
    if repo_has_trailing_escalations && row.depth == 0 && row.is_last {
        row.is_last = false;
    }
    let agent = &repo.agents[agent_idx];
    let agent_style = if selected {
        style
    } else if agent.status == Status::BranchOnly {
        THEME.status_style(Status::BranchOnly)
    } else {
        style
    };
    let active = agent
        .subagents
        .iter()
        .filter(|subagent| subagent.status == SubagentStatus::Running)
        .count();
    let escalations = app.escalations_for_agent(&agent.id);
    let mut indicator_state = IndicatorState::with_status(Some(agent.status));
    indicator_state.merging = app.is_merging_agent(&repo.path, &agent.id);
    indicator_state.merge_queued = app.merge_approval_queued(&agent.id);
    indicator_state.worktree_missing = agent.worktree_missing;
    indicator_state.merge_failed = agent.merge_error.is_some();
    indicator_state.needs_decision = agent.status != Status::Running
        && (app
            .overseer_snapshot
            .blocked_reason(app.locale, &agent.id)
            .is_some()
            || escalations.iter().any(|(_, item)| item.actionable()));
    indicator_state.worker_finished =
        agent.status != Status::Running && app.overseer_snapshot.worker_finished(&agent.id);
    indicator_state.merge_lifecycle = (agent.status != Status::Running)
        .then(|| app.overseer_snapshot.merge_lifecycle(&agent.id))
        .flatten();
    indicator_state.merged = app.overseer_snapshot.observed_merged(&agent.id);
    indicator_state.shell_active = agent.shell_working;
    indicator_state.mcp_active = agent.mcp_active;
    indicator_state.subagents_active = active;
    let primary = select(indicator_state);
    let right = indicator::supplementary_spans(
        primary,
        select_supplementary(indicator_state),
        selected,
        " ",
    );
    let has_children = agent
        .children
        .iter()
        .any(|child| super::super::actions::children::child_is_visible(agent, child));
    let handle = if !has_children {
        label::TreeHandle::Leaf
    } else if app.agent_children_expanded(repo_idx, agent_idx) {
        label::TreeHandle::Expanded
    } else {
        label::TreeHandle::Collapsed
    };
    let prefix = label::agent_row_prefix(
        marker,
        &row.ancestor_continues,
        row.is_last,
        handle,
        THEME.tree_structure_style(selected),
    );
    let title = match &agent.task_number {
        Some(number) => format!("#{number} {}", agent.title),
        None => agent.title.clone(),
    };
    let mut lines = vec![label::labeled_row(
        projects_width,
        prefix,
        primary,
        &title,
        agent_style,
        selected,
        app.started.elapsed(),
        right,
    )];
    let stopped = (agent.status != Status::Running)
        .then(|| app.overseer_snapshot.terminal_reason(&agent.id))
        .flatten();
    let held = (agent.status != Status::Running)
        .then(|| app.overseer_snapshot.held_reason(&agent.id))
        .flatten();
    let shown_escalation = escalation_line::newest(escalations.iter().map(|(_, item)| *item));
    let escalation_reasons = shown_escalation
        .iter()
        .map(|item| item.detail.as_str())
        .collect::<Vec<_>>();
    lines.extend(reason_line::build(
        agent,
        stopped.as_deref(),
        held.as_deref(),
        &escalation_reasons,
        &row,
        projects_width,
    ));
    lines.extend(escalation_line::build(
        escalations.into_iter().map(|(_, item)| item),
        &row,
        projects_width,
    ));
    lines
}
