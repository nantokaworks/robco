//! Inbox projections used to hang escalations under their owning tree rows.
//!
//! Every `(usize, &InboxItem)` pair carries the item's position in
//! `app.overseer_inbox` itself — NOT a position within the filtered result.
//! The index is what the existing index-based actions
//! (`dismiss_inbox_item(index)`, `approve_inbox(index)`) are called with, so
//! a refactor that renumbered filtered results would silently target the
//! wrong Inbox item.

use super::{App, inbox::InboxItem};

impl App {
    /// Escalations originating from `agent_id`, preserving Inbox list order.
    #[allow(dead_code, reason = "consumed by later leaves of dropr #580")]
    pub(crate) fn escalations_for_agent(&self, agent_id: &str) -> Vec<(usize, &InboxItem)> {
        self.overseer_inbox
            .iter()
            .enumerate()
            .filter(|(_, item)| item.agent_id.as_deref() == Some(agent_id))
            .collect()
    }

    /// Repo-level escalations not owned by any agent still in the registry.
    #[allow(dead_code, reason = "consumed by later leaves of dropr #580")]
    pub(crate) fn escalations_for_repo(&self, repo_label: &str) -> Vec<(usize, &InboxItem)> {
        self.overseer_inbox
            .iter()
            .enumerate()
            .filter(|(_, item)| item.repo.as_deref() == Some(repo_label))
            .filter(|(_, item)| {
                item.agent_id.as_deref().is_none_or(|agent_id| {
                    !self
                        .registry
                        .repos
                        .iter()
                        .flat_map(|repo| &repo.agents)
                        .any(|agent| agent.id == agent_id)
                })
            })
            .collect()
    }

    /// Escalations that are not associated with any repository.
    #[allow(dead_code, reason = "consumed by later leaves of dropr #580")]
    pub(crate) fn global_escalations(&self) -> Vec<(usize, &InboxItem)> {
        self.overseer_inbox
            .iter()
            .enumerate()
            .filter(|(_, item)| item.repo.is_none())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::{
        config::Config,
        registry::Registry,
        ui::{inbox::InboxKind, test_support},
    };

    fn item(agent_id: Option<&str>, repo: Option<&str>, second: u32) -> InboxItem {
        InboxItem {
            kind: InboxKind::Escalation,
            repo: repo.map(str::to_owned),
            agent_id: agent_id.map(str::to_owned),
            target_session: None,
            target_id: format!("#{second}"),
            label: format!("#{second}"),
            detail: "needs user".into(),
            at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, second).unwrap(),
            pr_url: None,
            pr_facts: None,
            sentence: None,
        }
    }

    fn app(registry: Registry, items: Vec<InboxItem>) -> App {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.registry = registry;
        app.overseer_inbox = items;
        app
    }

    #[test]
    fn agent_lookup_preserves_list_order_and_original_indices() {
        let app = app(
            Registry::default(),
            vec![
                item(Some("one"), Some("robco"), 3),
                item(Some("two"), Some("robco"), 2),
                item(Some("one"), Some("robco"), 1),
            ],
        );

        let rows = app.escalations_for_agent("one");
        assert_eq!(
            rows.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            [0, 2]
        );
        assert_eq!(rows[0].1.target_id, "#3");
        assert_eq!(rows[1].1.target_id, "#1");
    }

    #[test]
    fn repo_lookup_keeps_unowned_and_unregistered_agent_items() {
        let temp = tempfile::tempdir().unwrap();
        let repo = test_support::repo(
            temp.path().join("robco"),
            vec![test_support::agent("known", temp.path().join("known"))],
        );
        let app = app(
            Registry {
                version: 1,
                repos: vec![repo],
            },
            vec![
                item(Some("known"), Some("robco"), 4),
                item(Some("gone"), Some("robco"), 3),
                item(None, Some("robco"), 2),
                item(Some("gone"), Some("other"), 1),
            ],
        );

        let rows = app.escalations_for_repo("robco");
        assert_eq!(
            rows.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            [1, 2]
        );
    }

    #[test]
    fn global_lookup_keeps_only_items_without_a_repo() {
        let app = app(
            Registry::default(),
            vec![
                item(None, None, 3),
                item(Some("gone"), Some("robco"), 2),
                item(Some("gone"), None, 1),
            ],
        );

        let rows = app.global_escalations();
        assert_eq!(
            rows.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            [0, 2]
        );
    }
}
