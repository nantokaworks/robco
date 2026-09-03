use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::model::{Selection, Status};
use crate::subagents::SubagentStatus;

use super::{App, layout, theme::DEFAULT as THEME};
use indicator::{IndicatorState, select, select_supplementary};

mod footer;
mod hints;
mod host_chip;
mod host_group;
pub(in crate::ui) mod indicator;
mod label;
mod launch_row;
pub(in crate::ui) mod overseer_frame;
mod reason_line;
mod remote_chat_row;
mod repo_row;

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection], message: Option<&str>) {
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body, app.overseer_frame_height());
    if app.overseer_visible {
        overseer_frame::draw(frame, app, panes.overseer);
    }
    let projects_width = panes.tree.width.saturating_sub(1);

    let mut lines = host_chip::lines(app, projects_width, app.started.elapsed());
    for (idx, item) in visible.iter().enumerate() {
        let selected = idx == app.selected;
        let marker = if selected { ">" } else { " " };
        let style = if selected {
            THEME.selection_style()
        } else {
            THEME.accent_style()
        };

        match *item {
            Selection::OverseerAi
            | Selection::OverseerCategory(_)
            | Selection::OverseerInbox(_)
            | Selection::DiscordChannel(_) => continue,
            Selection::RemoteControlAi(_) | Selection::RemoteDiscordChannel { .. } => {
                if let Some(line) = remote_chat_row::build(app, *item, selected, marker) {
                    lines.push(line);
                }
            }
            Selection::Repo(repo_idx) => {
                lines.extend(repo_row::build(
                    app,
                    repo_idx,
                    selected,
                    marker,
                    style,
                    projects_width,
                ));
                // Launches in flight for this repo (dropr:517) show right
                // under its own row, gated on expansion the same way its real
                // agent rows are.
                if app.expanded.get(repo_idx).copied().unwrap_or(true) {
                    let repo = &app.registry.repos[repo_idx];
                    lines.extend(launch_row::build(
                        app,
                        &repo.path,
                        !repo.agents.is_empty(),
                        projects_width,
                    ));
                }
            }
            Selection::Agent {
                repo: repo_idx,
                agent: agent_idx,
            } => {
                let repo = &app.registry.repos[repo_idx];
                let row = crate::model::agent_row(&repo.agents, agent_idx);
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
                let mut indicator_state = IndicatorState::with_status(Some(agent.status));
                indicator_state.merging = app.is_merging_agent(&repo.path, &agent.id);
                // Not gated on the agent being quiet, unlike the ledger-sourced
                // badges below: this is robco's own state, not a report the
                // worker left behind. The `OpenPrThenQueue` path in fact leaves
                // the worker running — it is writing the pull request body —
                // and that is exactly when the operator needs to see that the
                // merge half of the keypress was accepted too (dropr:545).
                indicator_state.merge_queued = app.merge_approval_queued(&agent.id);
                indicator_state.worktree_missing = agent.worktree_missing;
                indicator_state.merge_failed = agent.merge_error.is_some();
                // Gated on the agent not actually running: a worker that has
                // resumed real work (spinner motion) has moved past whatever
                // report put it in this state, and the pane is the more
                // current signal at that point.
                indicator_state.needs_decision = agent.status != Status::Running
                    && app
                        .overseer_snapshot
                        .blocked_reason(app.locale, &agent.id)
                        .is_some();
                // Same gating: a worker that has resumed real work has moved
                // past whatever `--kind done` it last reported.
                indicator_state.worker_finished = agent.status != Status::Running
                    && app.overseer_snapshot.worker_finished(&agent.id);
                // Same gating as `needs_decision` above: a worker that has
                // resumed real work has moved past whatever the ledger
                // still records for its last pull request.
                indicator_state.merge_lifecycle = (agent.status != Status::Running)
                    .then(|| app.overseer_snapshot.merge_lifecycle(&agent.id))
                    .flatten();
                // Only ever consulted while `agent.status == Status::Dead`
                // (`indicator::select` gates on its own `dead` flag), so no
                // extra gating is needed here the way the ledger-sourced
                // badges above need it.
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
                    .any(|child| super::actions::children::child_is_visible(agent, child));
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
                // The failure's own text, under the row that only badged it
                // (dropr:518); for a ledger entry parked in a terminal phase,
                // did not mark at all (dropr:524); or for one still open but
                // held on something that will not clear on its own
                // (dropr:529). Nothing when the agent has none of the three,
                // so a healthy tree keeps its height. Gated the same way the
                // ledger-sourced badges above are: a worker that has resumed
                // real work has moved past whatever the ledger still records
                // for it.
                let stopped = (agent.status != Status::Running)
                    .then(|| app.overseer_snapshot.terminal_reason(&agent.id))
                    .flatten();
                let held = (agent.status != Status::Running)
                    .then(|| app.overseer_snapshot.held_reason(&agent.id))
                    .flatten();
                lines.extend(reason_line::build(
                    agent,
                    stopped.as_deref(),
                    held.as_deref(),
                    &row,
                    projects_width,
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
            // "OTHER LOCATIONS" / "ORPHAN SESSIONS" are all-caps section-divider
            // chrome, the same family as "PROJECTS" and "OVERSEER" (out of
            // scope per the task) and "DROPR" / "HISTORY" / "PULL REQUESTS" in
            // the summary pane (left untranslated by #372) — kept structural
            // for consistency with that convention.
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
                        format!("  {}", host_group::short_path(&orphan.cwd)),
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

#[cfg(test)]
mod host_group_tests;
#[cfg(test)]
mod merge_queued_row_tests;
#[cfg(test)]
mod render_test_support;
#[cfg(test)]
mod task_number_tests;
#[cfg(test)]
mod tests;
