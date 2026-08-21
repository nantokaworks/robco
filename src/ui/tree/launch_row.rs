//! The launching-task placeholder row (dropr:517).
//!
//! A task the operator just fired shows here, in the tree, from the keypress
//! — before its worktree or tmux session exists. It renders straight off
//! `App::task_launch_jobs`, never the registry: nothing here is ever written
//! to disk, so a crash mid-launch leaves no ghost row behind, and the row
//! simply stops appearing once its job resolves — replaced by the real agent
//! row on success, or by nothing at all on failure.
//!
//! Deliberately not a [`crate::model::Selection`] variant: the operator's
//! cursor can never land on one of these, which is the whole point — a
//! launch must never steal focus, not when it starts and not when it ends.

use ratatui::text::Line;

use crate::model::Status;
use crate::ui::{App, actions::dropr_task_worker::TaskLaunchJob, theme::DEFAULT as THEME};

use super::indicator::{IndicatorState, select};
use super::label;

/// One row per launch this repository has in flight, sorted by display id so
/// the order is stable across redraws — `HashMap` iteration order is not.
/// `has_agents` decides whether the last placeholder draws as the last row
/// under the repo, or expects a real agent row right after it.
pub(super) fn build(
    app: &App,
    repo_path: &std::path::Path,
    has_agents: bool,
    projects_width: u16,
) -> Vec<Line<'static>> {
    let mut launches: Vec<&TaskLaunchJob> = app
        .task_launch_jobs
        .values()
        .filter(|job| job.repo_path == repo_path)
        .collect();
    launches.sort_by(|a, b| a.display_id.cmp(&b.display_id));
    let primary = select(IndicatorState::with_status(Some(Status::Running)));
    let last_index = launches.len().saturating_sub(1);
    launches
        .into_iter()
        .enumerate()
        .map(|(index, job)| {
            let is_last = !has_agents && index == last_index;
            let prefix = label::agent_row_prefix(
                " ",
                &[],
                is_last,
                label::TreeHandle::Leaf,
                THEME.tree_structure_style(false),
            );
            label::labeled_row(
                projects_width,
                prefix,
                primary,
                &job.title,
                THEME.hint_style(),
                false,
                app.started.elapsed(),
                Vec::new(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::actions::dropr_task_worker::test_job;

    fn row_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn no_jobs_draws_nothing() {
        let app_repo = std::path::PathBuf::from("/repo");
        // No `App` fixture is needed for the empty case, but `build` always
        // takes one — construct the smallest real one available in this crate.
        let temp = tempfile::tempdir().unwrap();
        let app = crate::ui::App::new(
            crate::registry::Registry::default(),
            crate::config::Config::default(),
            temp.path().into(),
        );

        let lines = build(&app, &app_repo, false, 40);

        assert!(lines.is_empty());
    }

    #[test]
    fn a_job_for_this_repo_draws_one_row_naming_the_task() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = crate::ui::App::new(
            crate::registry::Registry::default(),
            crate::config::Config::default(),
            temp.path().into(),
        );
        let (job, _sender) = test_job("#1", "#1 Fix the thing", "/repo".into(), "id-#1", "open");
        app.task_launch_jobs.insert("id-#1".to_string(), job);

        let lines = build(&app, std::path::Path::new("/repo"), false, 40);

        assert_eq!(lines.len(), 1);
        assert!(row_text(&lines[0]).contains("#1 Fix the thing"));
    }

    #[test]
    fn a_job_for_a_different_repo_is_not_drawn() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = crate::ui::App::new(
            crate::registry::Registry::default(),
            crate::config::Config::default(),
            temp.path().into(),
        );
        let (job, _sender) = test_job(
            "#1",
            "#1 Fix the thing",
            "/elsewhere".into(),
            "id-#1",
            "open",
        );
        app.task_launch_jobs.insert("id-#1".to_string(), job);

        let lines = build(&app, std::path::Path::new("/repo"), false, 40);

        assert!(lines.is_empty());
    }
}
