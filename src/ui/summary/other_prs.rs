//! The PULL REQUESTS block of a repository summary.
//!
//! Everything here is discovered by `crate::overseer::daemon::external_prs`,
//! not dispatched by Overseer — a Dependabot bump, a human's own branch,
//! another agent's. Unlike the DROPR and HISTORY blocks above it, this
//! section is omitted entirely when a repository has none: an empty section
//! would be noise on every repository that has never had a stray pull
//! request, and the acceptance criteria for dropr task #350 says so
//! explicitly.

use std::path::Path;

use ratatui::text::{Line, Span};

use crate::{
    locale::{Locale, fmt},
    overseer::other_prs::{OtherPr, OtherPrs},
    ui::theme::DEFAULT as THEME,
};

/// Rows listed before the rest are counted instead of listed — the same
/// scannability rationale as `HISTORY_DISPLAY_LIMIT` next door.
const DISPLAY_LIMIT: usize = 10;

pub(super) fn other_prs_section(
    other_prs: &OtherPrs,
    repo_path: &Path,
    width: u16,
    locale: Locale,
) -> Vec<Line<'static>> {
    let Some(repo) = other_prs
        .repos
        .get(&repo_path.to_string_lossy().into_owned())
    else {
        return Vec::new();
    };
    if repo.prs.is_empty() {
        return Vec::new();
    }
    let mut prs: Vec<&OtherPr> = repo.prs.iter().collect();
    prs.sort_by_key(|pr| pr.number);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "─".repeat(usize::from(width)),
            THEME.muted_style(),
        )),
        Line::from(Span::styled("PULL REQUESTS", THEME.accent_style())),
    ];
    lines.extend(prs.iter().take(DISPLAY_LIMIT).map(|pr| pr_line(pr)));
    let hidden = prs.len().saturating_sub(DISPLAY_LIMIT);
    if hidden > 0 {
        lines.push(Line::from(Span::styled(
            fmt(locale, "… and {} more", &[&hidden.to_string()]),
            THEME.muted_style(),
        )));
    }
    lines
}

/// `CLEAN` is the one state that needs no operator attention; every other
/// `mergeStateStatus` GitHub reports (`UNSTABLE`, `DIRTY`, `BLOCKED`, …) is
/// shown in the same failure color the HISTORY block uses for an unmerged
/// entry, so a pull request that needs a decision reads as red at a glance.
fn pr_line(pr: &OtherPr) -> Line<'static> {
    let state_style = if pr.mergeable_state.eq_ignore_ascii_case("CLEAN") {
        THEME.muted_style()
    } else {
        THEME.failure_style()
    };
    Line::from(vec![
        Span::raw(format!("#{}  ", pr.number)),
        Span::raw(pr.title.clone()),
        Span::styled(format!("  by {}", pr.author), THEME.muted_style()),
        Span::styled(format!("  {}", pr.mergeable_state), state_style),
    ])
}

#[cfg(test)]
#[path = "other_prs_tests.rs"]
mod tests;
