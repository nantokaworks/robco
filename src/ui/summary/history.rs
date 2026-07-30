//! The Overseer history block of a repository summary.
//!
//! Every other Overseer surface filters terminal entries out — the frame counts
//! live workers, the Inbox lists what still needs the operator — so the work
//! Overseer has actually finished in a repository was visible nowhere. This is
//! that record, shown where the operator already looks for repository context.
//!
//! The ledger is unbounded and holds every repository at once, so the section
//! filters by path and caps what it lists. A capped list that says it is capped
//! is fine; one that looks complete misinforms.

use std::path::Path;

use ratatui::text::{Line, Span};

use crate::{
    locale::{Locale, fmt, t},
    overseer::ledger::{Ledger, LedgerEntry, LedgerPhase, terminal},
    ui::theme::DEFAULT as THEME,
};

/// Entries the section lists before the rest are counted instead of listed.
///
/// The history is the last block in the summary and the preview pane scrolls,
/// so the cap is about keeping the block scannable rather than about fitting
/// the terminal. What it drops is the oldest, which is the least interesting.
const HISTORY_DISPLAY_LIMIT: usize = 10;

/// The HISTORY block, divider and heading included.
///
/// Always rendered, for the same reason the DROPR block is: a repository
/// Overseer has never touched and one whose section was simply dropped look
/// identical, and the operator cannot tell which they are looking at.
pub(super) fn history_section(
    ledger: &Ledger,
    repo_path: &Path,
    width: u16,
    locale: Locale,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "─".repeat(usize::from(width)),
            THEME.muted_style(),
        )),
        Line::from(Span::styled("HISTORY", THEME.accent_style())),
    ];
    lines.extend(history_lines(ledger, repo_path, locale));
    lines
}

/// The settled entries this repository owns, newest first.
///
/// Matching is on the absolute path dispatch recorded, not on the repository
/// name: two checkouts of the same repository are different working copies, and
/// listing one's history under the other would be a lie the operator cannot see
/// through.
fn history_lines(ledger: &Ledger, repo_path: &Path, locale: Locale) -> Vec<Line<'static>> {
    let mut settled: Vec<&LedgerEntry> = ledger
        .entries
        .iter()
        .filter(|entry| terminal(entry.phase) && Path::new(&entry.repo) == repo_path)
        .collect();
    // Entries that settled before `settled_at` existed fall back to when they
    // were dispatched — the wrong instant, but the right order, and it keeps
    // them from all collapsing to the bottom of the list.
    settled.sort_by_key(|entry| std::cmp::Reverse(entry.settled_at.unwrap_or(entry.dispatched_at)));

    if settled.is_empty() {
        return vec![Line::from(Span::styled(
            t(locale, "overseer has settled no tasks in this repo"),
            THEME.muted_style(),
        ))];
    }
    let mut lines: Vec<Line<'static>> = settled
        .iter()
        .take(HISTORY_DISPLAY_LIMIT)
        .map(|entry| entry_line(entry))
        .collect();
    let hidden = settled.len().saturating_sub(HISTORY_DISPLAY_LIMIT);
    if hidden > 0 {
        lines.push(Line::from(Span::styled(
            fmt(locale, "… and {} more", &[&hidden.to_string()]),
            THEME.muted_style(),
        )));
    }
    lines
}

/// One settled entry: what it was, how it ended, when, and where to look.
fn entry_line(entry: &LedgerEntry) -> Line<'static> {
    let when = entry
        .settled_at
        .unwrap_or(entry.dispatched_at)
        .format("%m-%d %H:%M")
        .to_string();
    let mut spans = vec![
        Span::styled(format!("{when}  "), THEME.muted_style()),
        Span::raw(format!("{}  ", entry.display_id)),
        Span::styled(entry.phase.label().to_string(), phase_style(entry.phase)),
    ];
    if let Some(reference) = pull_request_reference(entry) {
        spans.push(Span::styled(format!("  {reference}"), THEME.muted_style()));
    }
    Line::from(spans)
}

/// `merged` is the outcome the operator expects; the other two are the ones
/// they may still have to act on, so they read as failures rather than as
/// another shade of done.
fn phase_style(phase: LedgerPhase) -> ratatui::style::Style {
    match phase {
        LedgerPhase::Merged => THEME.muted_style(),
        _ => THEME.failure_style(),
    }
}

/// The pull request number, or the whole URL when it does not end in one.
///
/// The number is what identifies the change in conversation, and the row has no
/// width to spare for a full GitHub URL. A URL shaped differently than expected
/// is shown intact rather than mangled into a wrong number.
fn pull_request_reference(entry: &LedgerEntry) -> Option<String> {
    let url = entry.pr_url.as_deref()?;
    let number = url.rsplit('/').next().filter(|tail| {
        !tail.is_empty() && tail.chars().all(|character| character.is_ascii_digit())
    });
    Some(match number {
        Some(number) => format!("PR #{number}"),
        None => url.to_string(),
    })
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
