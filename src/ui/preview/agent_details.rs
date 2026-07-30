use ratatui::text::{Line, Span};

use crate::{
    model::{AgentNode, Status},
    ui::{App, theme::DEFAULT as THEME},
};

/// Extra detail lines spliced into an agent's Info pane summary: worktree
/// state, the last merge failure, and — once the AI session itself has gone
/// quiet — anything the Overseer ledger still has to say about the worker
/// (a decision it is waiting on, or where its pull request stands).
pub(in crate::ui) fn lines(app: &App, agent: &AgentNode) -> Vec<Line<'static>> {
    let mut details = Vec::new();
    if agent.worktree_missing {
        details.push(Line::from(vec![
            Span::styled("worktree missing: ", THEME.muted_style()),
            Span::styled(
                agent.worktree_path.display().to_string(),
                THEME.worktree_missing_style(false),
            ),
        ]));
    }
    if let Some(error) = &agent.merge_error {
        for (row, error_line) in error.split('\n').enumerate() {
            details.push(Line::from(vec![
                Span::styled(
                    if row == 0 {
                        "merge failed: "
                    } else {
                        "              "
                    },
                    THEME.muted_style(),
                ),
                Span::styled(error_line.to_string(), THEME.merge_failed_style(false)),
            ]));
        }
    }
    // Both gated the same way: a worker that has resumed real work (spinner
    // motion) has moved past whatever the ledger still records for it.
    if agent.status != Status::Running {
        if let Some(reason) = app.overseer_snapshot.blocked_reason(app.locale, &agent.id) {
            details.push(Line::from(vec![
                Span::styled("blocked: ", THEME.muted_style()),
                Span::styled(reason, THEME.needs_decision_style(false)),
            ]));
        }
        if let Some(lifecycle) = app.overseer_snapshot.merge_lifecycle(&agent.id) {
            let detail = app
                .overseer_snapshot
                .merge_hold_detail(app.locale, &agent.id)
                .unwrap_or_default();
            details.push(Line::from(vec![
                Span::styled("pull request: ", THEME.muted_style()),
                Span::styled(detail, THEME.merge_lifecycle_style(lifecycle)),
            ]));
        }
    }
    details
}
