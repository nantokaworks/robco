use ratatui::text::{Line, Span};

use crate::ui::{App, theme::DEFAULT as THEME};

use super::{escalation_line, label};

pub(super) fn build(
    app: &App,
    repo: usize,
    item: usize,
    selected: bool,
    marker: &str,
    width: u16,
    is_last: bool,
) -> Option<Line<'static>> {
    let repo = app.registry.repos.get(repo)?;
    let item = app.overseer_inbox.get(item)?;
    if item.repo.as_deref() != Some(repo.name.as_str()) {
        return None;
    }
    let style = if selected {
        THEME.selection_style()
    } else {
        THEME.muted_style()
    };
    let reason = escalation_line::row_reason(&item.detail)
        .map(|reason| format!(" — {reason}"))
        .unwrap_or_default();
    let mut spans = vec![
        label::leaf_row_prefix(marker, &[], is_last, THEME.tree_structure_style(selected)),
        Span::styled(format!("[{}] ", item.kind.code()), style),
        Span::styled(item.remedy().tag().to_string(), style),
        Span::styled(format!(" {}{reason}", item.target_id), style),
    ];
    label::trim_spans_to_width(&mut spans, usize::from(width));
    Some(Line::from(spans))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use crate::{
        config::Config,
        registry::Registry,
        ui::{
            App,
            inbox::{InboxItem, InboxKind},
            test_support,
            tree::render_test_support::{rendered_rows, row_containing},
        },
    };

    fn item(target_id: &str, second: u32) -> InboxItem {
        InboxItem {
            kind: InboxKind::Escalation,
            repo: Some("repo".into()),
            agent_id: Some(format!("gone-{second}")),
            target_session: None,
            target_id: target_id.into(),
            label: target_id.into(),
            detail: "worker blocked".into(),
            at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, second).unwrap(),
            pr_url: None,
            pr_facts: None,
            sentence: None,
        }
    }

    #[test]
    fn agent_and_two_escalations_keep_connectors_until_the_last_row() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            worktree_root: temp.path().into(),
            ..Config::default()
        };
        let repo = test_support::repo(
            temp.path().join("repo"),
            vec![test_support::agent("worker", temp.path().join("worker"))],
        );
        let mut app = App::new(
            Registry {
                version: 1,
                repos: vec![repo],
            },
            config,
            temp.path().into(),
        );
        app.overseer_visible = false;
        app.orphans.clear();
        app.overseer_inbox = vec![item("a1", 2), item("a2", 1)];
        app.set_repo_expanded(0, true);

        let rows = rendered_rows(&app);

        assert!(row_containing(&rows, "worker").starts_with("  ├── "));
        assert!(row_containing(&rows, "a1").starts_with("  ├── "));
        assert!(row_containing(&rows, "a2").starts_with("  └── "));
    }
}
