//! One repo's flattened rows: itself, its selectable dropr tasks, and its
//! agents (with their child worktrees). Split out of `ui::list` — which this
//! module's own `super` — to keep that file under this project's source file
//! size limit after dropr:470 added the dropr task rows.

use crate::model::{RepoNode, Selection};
use crate::ui::App;

use super::dropr_task_rows;

pub(super) fn push_repo_rows(
    app: &App,
    visible: &mut Vec<Selection>,
    repo_idx: usize,
    repo: &RepoNode,
) {
    visible.push(Selection::Repo(repo_idx));
    if !app.expanded.get(repo_idx).copied().unwrap_or(true) {
        return;
    }
    dropr_task_rows::push_rows(visible, repo_idx, repo);
    for (agent_idx, _) in crate::model::agent_order(&repo.agents) {
        visible.push(Selection::Agent {
            repo: repo_idx,
            agent: agent_idx,
        });
        if !app.agent_children_expanded(repo_idx, agent_idx) {
            continue;
        }
        for child in 0..repo.agents[agent_idx].children.len() {
            if !super::super::actions::children::child_is_visible(
                &repo.agents[agent_idx],
                &repo.agents[agent_idx].children[child],
            ) {
                continue;
            }
            visible.push(Selection::ChildWorktree {
                repo: repo_idx,
                agent: agent_idx,
                child,
            });
        }
    }
}
