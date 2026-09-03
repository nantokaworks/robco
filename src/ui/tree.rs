use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::model::Selection;
#[cfg(test)]
use crate::model::Status;

use super::{App, layout, theme::DEFAULT as THEME};
mod agent_row;
mod escalation_line;
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
mod repo_escalation_row;
mod repo_row;
use indicator::{IndicatorState, select};

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
            | Selection::OverseerAlert(_)
            | Selection::OverseerCategory(_)
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
            Selection::RepoEscalation { repo, item } => {
                let is_last = !matches!(
                    visible.get(idx + 1),
                    Some(Selection::RepoEscalation { repo: next_repo, .. }) if *next_repo == repo
                );
                if let Some(line) = repo_escalation_row::build(
                    app,
                    repo,
                    item,
                    selected,
                    marker,
                    projects_width,
                    is_last,
                ) {
                    lines.push(line);
                }
            }
            Selection::Agent {
                repo: repo_idx,
                agent: agent_idx,
            } => {
                lines.extend(agent_row::build(
                    app,
                    repo_idx,
                    agent_idx,
                    selected,
                    marker,
                    style,
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
