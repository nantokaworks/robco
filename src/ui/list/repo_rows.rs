//! One repo's flattened rows: itself and its agents (with their child
//! worktrees). Split out of `ui::list` — which this module's own `super` —
//! to keep that file under this project's source file size limit.
//!
//! A repo's dropr tasks are not rows here: walking past a repository must not
//! walk through its tasks too (dropr:475). They live in the repo's own INFO
//! preview instead, reached by drilling in — see `ui::DroprTaskFocus`.

use crate::model::{RepoNode, Selection};
use crate::ui::App;
use crate::ui::actions::remote_hosts::{HostConnection, HostSlot};

pub(super) fn remote_item_key(app: &App, selection: Selection) -> String {
    match selection {
        Selection::RemoteControlAi(host) => app.hosts.get(host).map_or_else(
            || "remote-control:missing".to_string(),
            |slot| format!("remote-control:{}", slot.label.ssh),
        ),
        Selection::RemoteDiscordChannel { host, channel } => app
            .hosts
            .get(host)
            .and_then(|slot| {
                let view = app.host_view(host)?;
                crate::ui::overseer::ordered_channel_ids(&view.discord_channels)
                    .get(channel)
                    .map(|id| format!("remote-discord:{}:{id}", slot.label.ssh))
            })
            .unwrap_or_else(|| "remote-discord:missing".to_string()),
        _ => unreachable!("remote chat selection required"),
    }
}

pub(super) fn push_remote_host_rows(
    app: &App,
    visible: &mut Vec<Selection>,
    host: usize,
    slot: &HostSlot,
) {
    for repo_idx in app
        .registry
        .repos
        .iter()
        .enumerate()
        .filter_map(|(index, repo)| (repo.host.as_ref() == Some(&slot.label)).then_some(index))
    {
        push_repo_rows(app, visible, repo_idx, &app.registry.repos[repo_idx]);
    }
    let Some(view) = app.host_view(host) else {
        return;
    };
    if view.connection != HostConnection::Connected {
        return;
    }
    visible.push(Selection::RemoteControlAi(host));
    let count = crate::ui::overseer::ordered_channel_ids(&view.discord_channels).len();
    visible.extend((0..count).map(|channel| Selection::RemoteDiscordChannel { host, channel }));
}

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
