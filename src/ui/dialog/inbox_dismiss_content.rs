//! Body lines for the "clear the overseer inbox?" confirmation — split out of
//! `content.rs` to keep that file under the size limit.

use ratatui::text::Line;

use super::hint_line;
use crate::locale::{Locale, fmt, t};
use crate::ui::inbox::InboxItem;

/// Above this many rows the dialog names each one; at or beyond it, the list
/// would run past a typical dialog's height, so it summarises by remedy tag
/// instead — the same `TAG repo target` identity the Inbox row itself shows,
/// so the dialog names exactly what the operator already recognises from the
/// list behind it, never the store files that hold it on disk.
const NAMED_LIST_MAX: usize = 6;

/// The whole body: how many items, what they are, and what clearing them
/// costs — no file names, since those are the implementation's vocabulary,
/// not the operator's (dropr:498).
pub(super) fn body(locale: Locale, count: usize, items: &[InboxItem]) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(fmt(
        locale,
        "hiding {} item(s):",
        &[&count.to_string()],
    ))];
    lines.extend(summary(items));
    lines.extend([
        Line::from(t(
            locale,
            "nothing on record is deleted, only removed from this list",
        )),
        Line::from(t(
            locale,
            "a hidden item returns only if the same target escalates again; \
             otherwise it stays hidden even if it still needs you",
        )),
        hint_line(locale, "enter clear   esc cancel"),
    ]);
    lines
}

fn summary(items: &[InboxItem]) -> Vec<Line<'static>> {
    if items.is_empty() {
        return Vec::new();
    }
    if items.len() <= NAMED_LIST_MAX {
        return items
            .iter()
            .map(|item| {
                let repo = item
                    .repo
                    .as_deref()
                    .map(|repo| format!("{repo} "))
                    .unwrap_or_default();
                Line::from(format!(
                    "  {} {repo}{}",
                    item.remedy().tag(),
                    item.target_id
                ))
            })
            .collect();
    }
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for item in items {
        let tag = item.remedy().tag();
        match counts.iter_mut().find(|(seen, _)| *seen == tag) {
            Some((_, n)) => *n += 1,
            None => counts.push((tag, 1)),
        }
    }
    let summary = counts
        .iter()
        .map(|(tag, n)| format!("{n} {tag}"))
        .collect::<Vec<_>>()
        .join(", ");
    vec![Line::from(format!("  {summary}"))]
}
