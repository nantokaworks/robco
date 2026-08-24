//! The DROPR task panel's non-row states: `answered == false` (fetching,
//! never queried, unavailable) and the "list is incomplete" footer for a
//! partial answer. Split out of `dropr_tasks.rs` to keep that file under this
//! project's source file size limit (dropr:543).

use ratatui::text::{Line, Span};

use crate::{
    dropr::DroprTaskFetch,
    locale::{Locale, t},
    ui::theme::DEFAULT as THEME,
};

/// The panel's `answered == false` state. One boolean (`answered`) used to
/// stand for two different situations — a fetch that failed, and a fetch
/// that simply has not come back yet — so this distinguishes three:
///
/// - a fetch is outstanding (including the very first one a freshly linked
///   workspace gets) — nothing has gone wrong, the answer just has not
///   arrived yet;
/// - no fetch is running and none ever reported a problem — never queried,
///   a different claim from "unavailable", which says a query ran and came
///   back empty-handed;
/// - a query ran and did not answer — genuinely unavailable.
///
/// See the dropr:543 decision scribble on this task for the full reasoning,
/// including why an in-flight refresh over an already-answered fetch earns
/// no line of its own here.
pub(super) fn unanswered_lines(
    fetch: &DroprTaskFetch,
    locale: Locale,
    fetch_in_flight: bool,
) -> Vec<Line<'static>> {
    if fetch_in_flight {
        return status_lines(t(locale, "fetching tasks…"));
    }
    if fetch.problems.is_empty() {
        return status_lines(t(locale, "tasks not checked yet"));
    }
    problem_lines(t(locale, "tasks unavailable"), &fetch.problems)
}

/// A single-line, non-alarming status in place of the task list — used for
/// the two `answered == false` cases that are not a failure (fetching, never
/// queried). Deliberately not styled as a failure: nothing has gone wrong.
fn status_lines(message: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(message.to_string(), THEME.muted_style())),
    ]
}

/// The block that keeps a short list from reading as a whole one.
pub(super) fn problem_lines(heading: &str, problems: &[String]) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(heading.to_string(), THEME.failure_style())),
    ];
    lines.extend(
        problems
            .iter()
            .map(|problem| Line::from(Span::styled(format!("! {problem}"), THEME.failure_style()))),
    );
    lines
}
