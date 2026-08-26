//! The repo row: name, agent count, dropr/indicator glyphs, and — when
//! collapsed — a rollup of its agents' statuses. Split out of `tree::draw`
//! because the repo row alone carries as much rendering logic as every other
//! row kind combined.

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::locale::t;
use crate::model::Status;
use crate::ui::{App, theme::DEFAULT as THEME};

use super::indicator::{self, IndicatorState, select, select_supplementary};
use super::label;

/// The repo row itself, plus a trailing "(no agents)" filler line when the
/// repo is expanded and empty.
pub(super) fn build(
    app: &App,
    repo_idx: usize,
    selected: bool,
    marker: &str,
    style: Style,
    projects_width: u16,
) -> Vec<Line<'static>> {
    let repo = &app.registry.repos[repo_idx];
    let expanded = app.expanded.get(repo_idx).copied().unwrap_or(true);
    let prefix = app.config.project_icon.marker(expanded);
    let title_style = style;
    let mut right = vec![Span::styled(
        format!(" {}", repo.agents.len()),
        if selected { style } else { THEME.hint_style() },
    )];
    if !app.repo_is_local(repo) {
        right.push(Span::styled(
            format!("  {}", super::short_path(&repo.path)),
            if selected { style } else { THEME.muted_style() },
        ));
    }
    let dropr_refresh = repo
        .dropr
        .as_ref()
        .is_some_and(|workspace| app.dropr_refresh_in_flight(&workspace.id));
    let mut indicator_state = IndicatorState::with_status(repo.main_status);
    indicator_state.shell_active = repo.main_shell_working;
    indicator_state.mcp_active = repo.main_mcp_active;
    indicator_state.subagents_active = repo.main_subagents_active;
    indicator_state.dropr_refresh = dropr_refresh;
    let primary = select(indicator_state);
    right.extend(indicator::supplementary_spans(
        primary,
        select_supplementary(indicator_state),
        selected,
        "  ",
    ));
    if !expanded && !repo.agents.is_empty() {
        collapsed_rollup(
            repo,
            &app.overseer_snapshot.ledger,
            selected,
            style,
            &mut right,
        );
    }
    let mut lines = vec![label::labeled_row(
        projects_width,
        vec![Span::styled(format!("{marker} {prefix} "), style)],
        primary,
        &repo.name,
        title_style,
        selected,
        app.started.elapsed(),
        right,
    )];
    // A launch in flight (dropr:517) is about to become this repo's first
    // agent, so "(no agents)" would misdescribe what the placeholder row
    // right below it is showing.
    let launching = app
        .task_launch_jobs
        .values()
        .any(|job| job.repo_path == repo.path);
    if expanded && repo.agents.is_empty() && !launching {
        // Six columns: where an agent row's own title would start (cursor +
        // separator + connector + separator), so the filler text sits where
        // an actual agent title would.
        lines.push(Line::from(Span::styled(
            format!(
                "      {}{}",
                label::AGENT_INDENT,
                t(app.locale, "(no agents)")
            ),
            THEME.muted_style(),
        )));
    }
    lines
}

/// The status a rollup counts an agent under: its own `Status::Dead`
/// unless the ledger has observed its pull request merged, in which case
/// it counts as `Status::Done` — the same substitution
/// `crate::ui::tree::indicator::select` makes for the row's own glyph, so a
/// collapsed repo's rollup never shows a merged-but-dead agent as an error
/// that an expanded row right next to it would not (dropr:563).
fn rollup_status(
    agent: &crate::model::AgentNode,
    ledger: &crate::overseer::ledger::Ledger,
) -> Status {
    if agent.status == Status::Dead && ledger.observed_merged(&agent.id) {
        Status::Done
    } else {
        agent.status
    }
}

/// Status-glyph rollup shown on a collapsed repo row in place of its
/// (invisible) agent rows.
fn collapsed_rollup(
    repo: &crate::model::RepoNode,
    ledger: &crate::overseer::ledger::Ledger,
    selected: bool,
    style: Style,
    right: &mut Vec<Span<'static>>,
) {
    let status_counts = [
        Status::Running,
        Status::Waiting,
        Status::Done,
        Status::Idle,
        Status::Dead,
        Status::BranchOnly,
    ]
    .map(|status| {
        (
            status,
            repo.agents
                .iter()
                .filter(|agent| rollup_status(agent, ledger) == status)
                .count(),
        )
    });
    // On a selected row every chunk joins the reversed selection bar (full
    // inversion, matching `indicator::primary_span`/`supplementary_spans`)
    // rather than keeping its per-status colour: `selection_bg` is the same
    // green as `Status::Running`, so a coloured-on-selection-background
    // scheme would make a running count vanish. The per-status colours stay
    // intact on the unselected row, which is the rollup's whole point.
    let status_style = |status| {
        if selected {
            THEME.selected_indicator_style()
        } else {
            THEME.status_style(status)
        }
    };
    let mut first = true;
    for (status, count) in status_counts {
        if count == 0 {
            continue;
        }
        right.push(Span::styled(if first { "  " } else { " · " }, style));
        right.push(Span::styled(
            format!("{count} {}", status.glyph()),
            status_style(status),
        ));
        first = false;
    }
    let missing_count = repo
        .agents
        .iter()
        .filter(|agent| agent.worktree_missing)
        .count();
    if missing_count > 0 {
        right.push(Span::styled(if first { "  " } else { " · " }, style));
        let missing_style = if selected {
            THEME.selected_indicator_style()
        } else {
            THEME.worktree_missing_style(false)
        };
        right.push(Span::styled(format!("{missing_count} ⌦"), missing_style));
    }
    let merge_failed_count = repo
        .agents
        .iter()
        .filter(|agent| agent.merge_error.is_some())
        .count();
    if merge_failed_count > 0 {
        right.push(Span::styled(if first { "  " } else { " · " }, style));
        let merge_failed_style = if selected {
            THEME.selected_indicator_style()
        } else {
            THEME.merge_failed_style(false)
        };
        right.push(Span::styled(
            format!("{merge_failed_count} merge-failed"),
            merge_failed_style,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{overseer::ledger::Ledger, ui::test_support};

    fn rollup_repo() -> crate::model::RepoNode {
        let dir = std::path::PathBuf::from("/tmp/repo_row_tests");
        let mut running = test_support::agent("running", dir.join("running"));
        running.status = Status::Running;
        let mut waiting = test_support::agent("waiting", dir.join("waiting"));
        waiting.status = Status::Waiting;
        let mut missing = test_support::agent("missing", dir.join("missing"));
        missing.worktree_missing = true;
        let mut failed = test_support::agent("failed", dir.join("failed"));
        failed.merge_error = Some("conflict".into());
        test_support::repo(dir, vec![running, waiting, missing, failed])
    }

    #[test]
    fn selected_rollup_has_no_background_gap() {
        let repo = rollup_repo();
        let mut right = Vec::new();
        collapsed_rollup(
            &repo,
            &Ledger::default(),
            true,
            THEME.selection_style(),
            &mut right,
        );

        // Status chunks, the worktree-missing chunk, and the merge-failed
        // chunk all previously fell through to a style with no `bg`, leaving
        // a hollow gap in the selection bar. Every span — separators
        // included — must now carry the selection background.
        assert!(!right.is_empty());
        for span in &right {
            assert_eq!(span.style.bg, Some(THEME.selection_bg));
        }

        // Full inversion (matching `indicator::primary_span`): every chunk —
        // including the `Running` count, which would otherwise be green on
        // the green selection background — loses its per-status colour on a
        // selected row in favour of the reversed selection foreground.
        for span in &right {
            assert_eq!(span.style.fg, Some(THEME.selection_fg));
        }
    }

    #[test]
    fn unselected_rollup_keeps_per_status_colours() {
        let repo = rollup_repo();
        let mut right = Vec::new();
        collapsed_rollup(
            &repo,
            &Ledger::default(),
            false,
            THEME.hint_style(),
            &mut right,
        );

        assert!(!right.is_empty());
        for span in &right {
            assert_eq!(
                span.style.bg, None,
                "unselected rollup must not gain a background"
            );
        }

        let running_chunk = right
            .iter()
            .find(|span| span.content.contains(Status::Running.glyph()))
            .expect("running chunk present");
        assert_eq!(running_chunk.style.fg, Some(THEME.running));

        let waiting_chunk = right
            .iter()
            .find(|span| span.content.contains(Status::Waiting.glyph()))
            .expect("waiting chunk present");
        assert_eq!(waiting_chunk.style.fg, Some(THEME.waiting));
    }
}

#[cfg(test)]
#[path = "repo_row_rollup_merged_tests.rs"]
mod rollup_merged_tests;
