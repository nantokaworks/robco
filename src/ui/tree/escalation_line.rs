use ratatui::text::{Line, Span};

use crate::{
    model::AgentRow,
    overseer::{discord::humanize, remedy::Move},
    ui::{inbox::InboxItem, theme::DEFAULT as THEME},
};

use super::label;

const CONNECTOR_TAIL: usize = 2;

pub(super) fn build<'a>(
    items: impl IntoIterator<Item = &'a InboxItem>,
    row: &AgentRow,
    projects_width: u16,
) -> Option<Line<'static>> {
    let items = items.into_iter().collect::<Vec<_>>();
    let item = newest(items.iter().copied())?;
    let remedy = item.remedy();
    let style = THEME.merge_hold_style(false);
    let tag_style = if remedy.step == Move::Watch {
        THEME.muted_style()
    } else {
        style
    };
    let reason = row_reason(&item.detail)
        .map(|reason| format!(" — {reason}"))
        .unwrap_or_default();
    let mut spans = vec![
        Span::styled(
            format!(
                "{}{}",
                label::continuation_prefix(&row.ancestor_continues, row.is_last),
                " ".repeat(CONNECTOR_TAIL)
            ),
            THEME.tree_structure_style(false),
        ),
        Span::styled(format!("[{}] ", item.kind.code()), style),
        Span::styled(remedy.tag().to_string(), tag_style),
        Span::styled(reason, style),
    ];
    let hidden = items.len() - 1;
    let suffix = (hidden > 0).then(|| format!(" +{hidden}"));
    let suffix_width = suffix.as_ref().map_or(0, String::len);
    label::trim_spans_to_width(
        &mut spans,
        usize::from(projects_width).saturating_sub(suffix_width),
    );
    if let Some(suffix) = suffix {
        spans.push(Span::styled(suffix, THEME.muted_style()));
    }
    label::trim_spans_to_width(&mut spans, usize::from(projects_width));
    (!spans.is_empty()).then(|| Line::from(spans))
}

pub(super) fn newest<'a>(items: impl IntoIterator<Item = &'a InboxItem>) -> Option<&'a InboxItem> {
    items.into_iter().max_by(|left, right| {
        left.actionable()
            .cmp(&right.actionable())
            .then_with(|| left.at.cmp(&right.at))
    })
}

/// DELIBERATE temporary copy of `ui::overseer::inbox_rows::row_reason`.
/// Leaf #585 (dropr task 10mZwhUrBNq5wY64l5496) deletes `inbox_rows.rs`; keep
/// this local until then rather than consolidating code around a dying module.
pub(super) fn row_reason(detail: &str) -> Option<&str> {
    let first_line = detail.lines().next().unwrap_or(detail).trim();
    if first_line.is_empty() {
        return None;
    }
    Some(humanize::static_sentence(first_line).unwrap_or(first_line))
}

pub(super) fn same_first_line(left: &str, right: &str) -> bool {
    row_reason(left).is_some_and(|left| row_reason(right) == Some(left))
}

#[cfg(test)]
#[path = "escalation_line_tests.rs"]
mod tests;
