use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::model::{ManagementMode, Selection, Status};
use crate::subagents::SubagentStatus;

use super::{App, layout, theme::DEFAULT as THEME};
use indicator::{IndicatorState, select, select_supplementary};

mod footer;
mod hints;
mod indicator;
mod label;
pub(in crate::ui) mod overseer_frame;
mod repo_row;

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection], message: Option<&str>) {
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body, app.overseer_frame_height());
    if app.overseer_visible {
        overseer_frame::draw(frame, app, panes.overseer);
    }
    let projects_width = panes.tree.width.saturating_sub(1);

    let mut lines = vec![Line::from(Span::styled(
        "PROJECTS",
        THEME.accent_bold_style(),
    ))];
    for (idx, item) in visible.iter().enumerate() {
        let selected = idx == app.selected;
        let marker = if selected { ">" } else { " " };
        let style = if selected {
            THEME.selection_style()
        } else {
            THEME.accent_style()
        };

        match *item {
            Selection::OverseerCategory(_) | Selection::OverseerInbox(_) => continue,
            Selection::Repo(repo_idx) => {
                lines.extend(repo_row::build(
                    app,
                    repo_idx,
                    selected,
                    marker,
                    style,
                    projects_width,
                ));
            }
            Selection::Agent {
                repo: repo_idx,
                agent: agent_idx,
            } => {
                let repo = &app.registry.repos[repo_idx];
                let row = crate::model::agent_row(&repo.agents, agent_idx);
                let agent = &repo.agents[agent_idx];
                let repo_unmanaged = repo.management == ManagementMode::Manual;
                let agent_style = if selected {
                    style
                } else if agent.status == Status::BranchOnly {
                    THEME.status_style(Status::BranchOnly)
                } else if repo_unmanaged {
                    THEME.muted_style()
                } else {
                    style
                };
                let active = agent
                    .subagents
                    .iter()
                    .filter(|subagent| subagent.status == SubagentStatus::Running)
                    .count();
                let mut indicator_state = IndicatorState::with_status(Some(agent.status));
                indicator_state.merging = app.is_merging_agent(&repo.path, &agent.id);
                indicator_state.worktree_missing = agent.worktree_missing;
                indicator_state.merge_failed = agent.merge_error.is_some();
                // Gated on the agent not actually running: a worker that has
                // resumed real work (spinner motion) has moved past whatever
                // report put it in this state, and the pane is the more
                // current signal at that point.
                indicator_state.needs_decision = agent.status != Status::Running
                    && app.overseer_snapshot.blocked_reason(&agent.id).is_some();
                // Same gating as `needs_decision` above: a worker that has
                // resumed real work has moved past whatever the ledger
                // still records for its last pull request.
                indicator_state.merge_lifecycle = (agent.status != Status::Running)
                    .then(|| app.overseer_snapshot.merge_lifecycle(&agent.id))
                    .flatten();
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
                    .any(|child| super::actions::children::child_is_visible(agent, child));
                let handle = if !has_children {
                    label::TreeHandle::Leaf
                } else if app.agent_children_expanded(repo_idx, agent_idx) {
                    label::TreeHandle::Expanded
                } else {
                    label::TreeHandle::Collapsed
                };
                // Blank out a marker that only repeats what the repo row above
                // already shows, so the ones that remain are the agents whose
                // management actually diverges from their repo's.
                let agent_marker =
                    label::ManagementMarker::of(agent.parent_agent_id.as_deref(), agent.management)
                        .unless_matching(label::ManagementMarker::of_repo(repo.management));
                let prefix = label::agent_row_prefix(
                    marker,
                    agent_marker,
                    &row.ancestor_continues,
                    row.is_last,
                    handle,
                    THEME.tree_structure_style(selected),
                    THEME.management_marker_style(selected),
                );
                let title = match &agent.task_number {
                    Some(number) => format!("#{number} {}", agent.title),
                    None => agent.title.clone(),
                };
                lines.push(label::labeled_row(
                    projects_width,
                    prefix,
                    primary,
                    &title,
                    agent_style,
                    selected,
                    app.started.elapsed(),
                    right,
                ));
            }
            Selection::ChildWorktree {
                repo,
                agent: agent_idx,
                child: child_idx,
            } => {
                let repo = &app.registry.repos[repo];
                let row = crate::model::agent_row(&repo.agents, agent_idx);
                let agent = &repo.agents[agent_idx];
                let child = &agent.children[child_idx];
                let label = child.branch.as_deref().unwrap_or_else(|| {
                    child
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("worktree")
                });
                let child_style = if selected { style } else { THEME.hint_style() };
                // The child-worktree list hangs one level below the agent, so
                // its own guide column reads whether the agent has a later
                // sibling; "last" is computed among the *visible* siblings the
                // same filter the row loop above uses, not the raw list.
                let mut ancestor_continues = row.ancestor_continues.clone();
                ancestor_continues.push(!row.is_last);
                let child_is_last = !agent.children[child_idx + 1..]
                    .iter()
                    .any(|sibling| super::actions::children::child_is_visible(agent, sibling));
                // Same layering as the agent row: connector dim, branch name content.
                let mut spans = vec![
                    label::leaf_row_prefix(
                        marker,
                        &ancestor_continues,
                        child_is_last,
                        THEME.tree_structure_style(selected),
                    ),
                    Span::styled(label.to_string(), child_style),
                ];
                if child.clean == Some(false) {
                    spans.push(Span::styled(" *", child_style));
                }
                if child.tmux_session.is_some() {
                    spans.push(Span::styled(" ⌁", child_style));
                }
                lines.push(Line::from(spans));
            }
            Selection::OtherHeader => {
                let count = app.other_location_repos().len();
                let arrow = if app.other_collapsed { "▸" } else { "▾" };
                let noun = if count == 1 { "repo" } else { "repos" };
                let header_style = if selected { style } else { THEME.hint_style() };
                lines.push(Line::from(Span::styled(
                    format!("{marker} {arrow} OTHER LOCATIONS ({count} {noun})"),
                    header_style,
                )));
            }
            Selection::OrphanHeader => {
                let count = app.orphans.len();
                let arrow = if app.orphans_collapsed { "▸" } else { "▾" };
                let noun = if count == 1 { "session" } else { "sessions" };
                let header_style = if selected { style } else { THEME.hint_style() };
                lines.push(Line::from(Span::styled(
                    format!("{marker} {arrow} ORPHAN SESSIONS ({count} {noun})"),
                    header_style,
                )));
            }
            Selection::Orphan(orphan_idx) => {
                let Some(orphan) = app.orphans.get(orphan_idx) else {
                    continue;
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{marker}   "), style),
                    Span::styled(orphan.name.clone(), style),
                    Span::styled(
                        format!("  {}", short_path(&orphan.cwd)),
                        if selected { style } else { THEME.muted_style() },
                    ),
                ]));
            }
        }
    }

    let tree = Paragraph::new(lines).style(THEME.accent_style());
    let projects_area = Rect {
        width: projects_width,
        ..panes.tree
    };
    frame.render_widget(tree, projects_area);

    footer::draw(frame, app, root.footer, message);
}

fn short_path(path: &std::path::Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod render_test_support;
#[cfg(test)]
mod task_number_tests;
#[cfg(test)]
mod tests;
