//! One repo's flattened rows: itself and its agents (with their child
//! worktrees). Split out of `ui::list` — which this module's own `super` —
//! to keep that file under this project's source file size limit.
//!
//! A repo's dropr tasks are not rows here: walking past a repository must not
//! walk through its tasks too (dropr:475). They live in the repo's own INFO
//! preview instead, reached by drilling in — see `ui::DroprTaskFocus`.

use crate::model::{RepoNode, Selection};
use crate::ui::App;
pub(super) use crate::ui::actions::remote_hosts::HostConnection;
use crate::ui::actions::remote_hosts::HostSlot;

pub(super) fn remote_item_key(app: &App, selection: Selection) -> String {
    match selection {
        Selection::RemoteControlAi(host) => app.hosts.get(host).map_or_else(
            || "remote-control:missing".to_string(),
            |slot| format!("remote-control:{}", slot.label.ssh),
        ),
        Selection::RemoteHostError(host) => app.hosts.get(host).map_or_else(
            || "remote-host-error:missing".to_string(),
            |slot| format!("remote-host-error:{}", slot.label.ssh),
        ),
        Selection::HostHeader(host) => app.hosts.get(host).map_or_else(
            || "host-header:missing".to_string(),
            |slot| format!("host-header:{}", slot.label.ssh),
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
    let Some(view) = app.host_view(host) else {
        return;
    };
    if view.connection != HostConnection::Connected {
        return;
    }
    for repo_idx in app
        .registry
        .repos
        .iter()
        .enumerate()
        .filter_map(|(index, repo)| (repo.host.as_ref() == Some(&slot.label)).then_some(index))
    {
        push_repo_rows(app, visible, repo_idx, &app.registry.repos[repo_idx]);
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
    if repo.host.is_some() {
        return;
    }
    // `InboxItem.repo` is the registry label. Carrying the full path would
    // touch shared aggregation, deliberately out of this leaf's scope, so a
    // duplicate label belongs to the first matching local registry row only.
    let first_match = app
        .registry
        .repos
        .iter()
        .position(|candidate| candidate.host.is_none() && candidate.name == repo.name);
    if first_match == Some(repo_idx) {
        visible.extend(
            app.escalations_for_repo(&repo.name)
                .into_iter()
                .map(|(item, _)| Selection::RepoEscalation {
                    repo: repo_idx,
                    item,
                }),
        );
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::{
        config::Config,
        model::HostLabel,
        registry::Registry,
        ui::{
            inbox::{InboxItem, InboxKind},
            test_support,
        },
    };

    fn escalation() -> InboxItem {
        InboxItem {
            kind: InboxKind::Escalation,
            repo: Some("same".into()),
            agent_id: Some("gone".into()),
            target_session: None,
            target_id: "alert".into(),
            label: "alert".into(),
            detail: "worker blocked".into(),
            at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            pr_url: None,
            pr_facts: None,
            sentence: None,
        }
    }

    #[test]
    fn duplicate_labels_assign_escalation_to_first_local_repo_only() {
        let temp = tempfile::tempdir().unwrap();
        let mut first = test_support::repo(temp.path().join("one"), Vec::new());
        let mut second = test_support::repo(temp.path().join("two"), Vec::new());
        let mut remote = test_support::repo(temp.path().join("remote"), Vec::new());
        first.name = "same".into();
        second.name = "same".into();
        remote.name = "same".into();
        remote.host = Some(HostLabel {
            name: "remote".into(),
            ssh: "remote".into(),
        });
        let mut app = App::new(
            Registry {
                version: 1,
                repos: vec![first, second, remote],
            },
            Config::default(),
            temp.path().into(),
        );
        app.overseer_inbox = vec![escalation()];
        app.expanded = vec![true; 3];
        let mut visible = Vec::new();

        for repo in 0..3 {
            push_repo_rows(&app, &mut visible, repo, &app.registry.repos[repo]);
        }

        let escalations = visible
            .iter()
            .filter_map(|selection| match selection {
                Selection::RepoEscalation { repo, item } => Some((*repo, *item)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(escalations, [(0, 0)]);
        let key = app.item_key(Selection::RepoEscalation { repo: 0, item: 0 });
        assert!(key.contains(&app.registry.repos[0].path.display().to_string()));
        assert!(!key.contains(&app.registry.repos[1].path.display().to_string()));
    }

    #[test]
    fn remote_repo_is_not_reorderable_across_host_boundaries() {
        let labels = ["first", "second"].map(|ssh| HostLabel {
            name: ssh.into(),
            ssh: ssh.into(),
        });
        let repos = labels
            .iter()
            .enumerate()
            .map(|(index, host)| {
                let mut repo = test_support::repo(format!("/srv/{index}").into(), Vec::new());
                repo.host = Some(host.clone());
                repo
            })
            .collect();
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(
            Registry { version: 1, repos },
            Config::default(),
            temp.path().into(),
        );
        app.overseer_visible = false;
        app.orphans.clear();
        app.expanded = vec![true, true];
        app.hosts = labels.into_iter().map(HostSlot::connected).collect();
        app.sync_remote_host_views();
        app.selected = 1;

        app.move_selected_repo(1);

        assert_eq!(
            app.visible(),
            vec![
                Selection::HostHeader(0),
                Selection::Repo(0),
                Selection::RemoteControlAi(0),
                Selection::HostHeader(1),
                Selection::Repo(1),
                Selection::RemoteControlAi(1),
            ]
        );
    }
}
