use std::fs;

use super::*;
use crate::{agent, config::Config, model::ChildWorktree, registry::Registry};

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

#[test]
fn overseer_visibility_requires_both_daemon_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let pidfile = temp.path().join("overseer.pid");
    let ledger = temp.path().join("ledger.json");
    assert!(!overseer_artifacts_exist(&pidfile, &ledger));
    fs::write(&pidfile, "123").unwrap();
    assert!(!overseer_artifacts_exist(&pidfile, &ledger));
    fs::write(&ledger, "{}").unwrap();
    assert!(overseer_artifacts_exist(&pidfile, &ledger));
    fs::remove_file(&pidfile).unwrap();
    assert!(!overseer_artifacts_exist(&pidfile, &ledger));
}

#[test]
fn overseer_categories_are_always_listed_and_the_header_is_not_a_row() {
    let mut app = test_app();
    app.set_overseer_visibility(true);
    // Ignore any live robco tmux sessions the host discovers as orphans so the
    // tree contents are deterministic across environments.
    app.orphans = Vec::new();
    let mut expected = vec![Selection::OverseerAi];
    expected.extend(OverseerCategory::ALL.map(Selection::OverseerCategory));
    assert_eq!(app.visible(), expected);
}

fn inbox_item(target_id: &str) -> crate::ui::inbox::InboxItem {
    crate::ui::inbox::InboxItem {
        kind: crate::ui::inbox::InboxKind::Escalation,
        repo: None,
        target_session: Some("robco-agent-1".into()),
        target_id: target_id.into(),
        label: format!("{target_id} — escalated"),
        detail: format!("{target_id} escalated"),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }
}

#[test]
fn expanding_the_inbox_lists_its_items_as_rows_and_collapsing_takes_them_back() {
    let mut app = test_app();
    app.set_overseer_visibility(true);
    app.orphans = Vec::new();
    app.overseer_inbox = vec![inbox_item("#1"), inbox_item("#2")];

    // Collapsed, the control AI row and the categories are still the only
    // OVERSEER rows: an item the operator cannot see is not one the cursor can
    // land on.
    let mut categories = vec![Selection::OverseerAi];
    categories.extend(OverseerCategory::ALL.map(Selection::OverseerCategory));
    assert_eq!(app.visible(), categories);

    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);
    // +1: the control AI row sits ahead of every category in `categories`.
    let inbox_row = OverseerCategory::Inbox.index() + 1;
    assert_eq!(
        app.visible(),
        [
            &categories[..=inbox_row],
            &[Selection::OverseerInbox(0), Selection::OverseerInbox(1)],
            &categories[inbox_row + 1..],
        ]
        .concat()
    );

    // Collapsing takes the item rows away and leaves the cursor on a real row
    // rather than past the end of the list — never `None`, and never a stale
    // `OverseerInbox` reference into a row that no longer exists.
    app.selected = inbox_row + 2;
    app.set_overseer_category_expanded(OverseerCategory::Inbox, false);
    assert_eq!(app.visible(), categories);
    assert!(matches!(
        app.selected_item(),
        Some(Selection::OverseerCategory(_))
    ));
}

#[test]
fn app_overseer_frame_height_tracks_content_within_bounds() {
    let mut app = test_app();
    app.set_overseer_visibility(true);
    let content = crate::ui::tree::overseer_frame::content_lines(&app);
    assert_eq!(
        app.overseer_frame_height(),
        crate::ui::layout::overseer_frame_height(content.lines.len())
    );

    for category in OverseerCategory::ALL {
        app.set_overseer_category_expanded(category, true);
    }
    assert!(app.overseer_frame_height() > crate::ui::layout::OVERSEER_FRAME_MIN_HEIGHT);
    assert!(app.overseer_frame_height() <= crate::ui::layout::OVERSEER_FRAME_MAX_HEIGHT);
}

#[test]
fn selection_identity_survives_overseer_row_toggle() {
    let temp = tempfile::tempdir().unwrap();
    let repo_path = temp.path().join("repo");
    let repo = serde_json::from_value(serde_json::json!({
        "path": repo_path,
        "name": "repo",
        "remote_url": null,
        "agents": []
    }))
    .unwrap();
    let registry = Registry {
        version: 1,
        repos: vec![repo],
    };
    let mut app = App::new(registry, Config::default(), temp.path().into());
    app.set_overseer_visibility(false);
    assert!(matches!(app.selected_item(), Some(Selection::Repo(0))));

    app.set_overseer_visibility(true);
    assert_eq!(app.selected, 6);
    assert!(matches!(app.selected_item(), Some(Selection::Repo(0))));
    // Moving up off the first repo row lands on the last OVERSEER category —
    // never on the header, which is no longer a row.
    app.move_selection_up();
    assert_eq!(
        app.selected_item(),
        Some(Selection::OverseerCategory(OverseerCategory::Discord))
    );
    app.selected = 0;
    assert_eq!(app.selected_item(), Some(Selection::OverseerAi));

    app.set_overseer_visibility(false);
    assert_eq!(app.selected, 0);
    assert!(matches!(app.selected_item(), Some(Selection::Repo(0))));
}

#[test]
fn connected_remote_hosts_list_global_chats_after_their_repos() {
    use crate::{
        model::HostLabel, overseer::discord_channels::DiscordChannels,
        ui::actions::remote_hosts::HostSlot,
    };
    let connected = HostLabel {
        name: "Prod".into(),
        ssh: "prod".into(),
    };
    let failed = HostLabel {
        name: "Down".into(),
        ssh: "down".into(),
    };
    let remote_repo = |name: &str, host: HostLabel| {
        let mut repo: crate::model::RepoNode = serde_json::from_value(serde_json::json!({
            "path": format!("/srv/{name}"), "name": name, "remote_url": null, "pinned": true
        }))
        .unwrap();
        repo.host = Some(host);
        repo
    };
    let channels: DiscordChannels = serde_json::from_value(serde_json::json!({"channels": {
        "42": {"first_seen_at":"2025-01-01T00:00:00Z","last_active_at":"2025-01-01T00:00:00Z",
            "turn_count":1,"status":"idle","last_error":null,"channel_name":"ops"}
    }}))
    .unwrap();
    let mut app = test_app();
    app.overseer_visible = false;
    app.orphans.clear();
    app.registry.repos = vec![
        remote_repo("one", connected.clone()),
        remote_repo("two", failed.clone()),
    ];
    app.expanded = vec![true; 2];
    app.hosts = vec![
        HostSlot::connected_with_chats(
            connected.clone(),
            Some(crate::model::Status::Idle),
            channels,
            true,
        ),
        HostSlot::failed(failed, "offline"),
    ];
    app.sync_remote_host_views();

    assert_eq!(
        app.visible(),
        vec![
            Selection::Repo(0),
            Selection::RemoteControlAi(0),
            Selection::RemoteDiscordChannel {
                host: 0,
                channel: 0
            },
            Selection::Repo(1),
        ]
    );

    app.selected = 2;
    let key = app.item_key(app.selected_item().unwrap());
    app.registry.repos.insert(0, remote_repo("new", connected));
    app.expanded.insert(0, true);
    app.restore_selection(Some(key.clone()));
    assert_eq!(app.item_key(app.selected_item().unwrap()), key);
}

#[test]
fn agent_children_default_collapsed_expand_and_hide_when_merged() {
    let temp = tempfile::tempdir().unwrap();
    let repo_path = temp.path().join("repos/repo");
    let owner_path = temp.path().join("worktrees/nex_task-384");
    let slot_path = temp.path().join("worktrees/nex_task-384_slot_snap");
    fs::create_dir_all(&repo_path).unwrap();
    fs::create_dir_all(&owner_path).unwrap();
    fs::create_dir_all(&slot_path).unwrap();
    let mut repo = serde_json::from_value(serde_json::json!({
        "path": repo_path,
        "name": "repo",
        "remote_url": null,
        "agents": []
    }))
    .unwrap();
    let mut owner = agent::adopt_worktree(
        &repo,
        &Config::default(),
        owner_path,
        Some("nex/task-384".into()),
        None,
        None,
        None,
    );
    owner.children.push(ChildWorktree {
        path: slot_path,
        branch: Some("slot/task-386-snap".into()),
        head: None,
        clean: Some(true),
        ahead_behind: Some((0, 1)),
        tmux_session: None,
        modified_at: None,
    });
    repo.agents.push(owner);
    let registry = Registry {
        version: 1,
        repos: vec![repo],
    };
    let config = Config {
        worktree_root: temp.path().join("worktrees"),
        ..Config::default()
    };
    let mut app = App::new(registry, config, temp.path().join("repos"));
    app.set_overseer_visibility(false);

    assert_eq!(app.visible().len(), 2);
    app.set_agent_children_expanded(0, 0, true);
    assert!(matches!(
        app.visible().last(),
        Some(Selection::ChildWorktree { .. })
    ));

    let other_path = temp.path().join("worktrees/nex_task-300");
    fs::create_dir_all(&other_path).unwrap();
    let other = agent::adopt_worktree(
        &app.registry.repos[0],
        &Config::default(),
        other_path,
        Some("nex/task-300".into()),
        None,
        None,
        None,
    );
    app.registry.repos[0].agents.insert(0, other);
    assert!(app.agent_children_expanded(0, 1));

    app.registry.repos[0].agents[1].children[0].ahead_behind = Some((0, 0));
    assert!(
        app.visible()
            .iter()
            .any(|row| matches!(row, Selection::ChildWorktree { .. }))
    );

    app.registry.repos[0].agents[1].children[0].ahead_behind = Some((1, 0));
    assert!(
        !app.visible()
            .iter()
            .any(|row| matches!(row, Selection::ChildWorktree { .. }))
    );

    let recreated = app.registry.repos[0].agents.remove(1);
    app.restore_selection(None);
    app.registry.repos[0].agents.push(recreated);
    assert!(!app.agent_children_expanded(0, 1));
}
