use chrono::Utc;

use crate::{
    config::Config,
    registry::Registry,
    ui::{
        App,
        inbox::{InboxItem, InboxKind},
        test_support,
    },
};

#[test]
fn escalated_agent_info_includes_guidance_and_reason() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::default();
    let agent = test_support::agent("agt-a", config.worktree_root.join("agt-a"));
    let registry = Registry {
        version: 1,
        repos: vec![test_support::repo(temp.path().join("repo"), vec![agent])],
    };
    let mut app = App::new(registry, config, temp.path().into());
    app.overseer_inbox = vec![InboxItem {
        kind: InboxKind::Escalation,
        repo: Some("repo".into()),
        agent_id: Some("agt-a".into()),
        target_session: Some("robco_agt-a".into()),
        target_id: "#582".into(),
        label: "merge_state:dirty".into(),
        detail: "merge_state:dirty\nbase is main".into(),
        at: Utc::now(),
        pr_url: Some("https://github.com/nantokaworks/robco/pull/582".into()),
        pr_facts: None,
        sentence: None,
    }];

    let rendered = super::lines(&app, &app.registry.repos[0].agents[0])
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert!(
        rendered.iter().any(|line| line.starts_with("remedy: ")),
        "{rendered:?}"
    );
    assert!(rendered.iter().any(|line| line == "next step"));
    assert!(rendered.iter().any(|line| line == "reason"));
    assert!(rendered.iter().any(|line| line == "merge_state:dirty"));
    assert!(rendered.iter().any(|line| line == "pull request: #582"));
}
