use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::model::{OverseerCategory, Selection, Status};

use super::{IndicatorState, escalation_line, indicator, label, select};
use crate::ui::{App, theme::DEFAULT as THEME};

const GAP: &str = "  ";

const DETAIL_INDENT: &str = "    ";

pub(in crate::ui) struct FrameContent {
    pub(in crate::ui) lines: Vec<Line<'static>>,
    pub(in crate::ui) selected_row: u16,
}

impl FrameContent {
    pub(in crate::ui) fn scroll_offset(&self, inner_height: u16) -> u16 {
        self.selected_row
            .saturating_add(1)
            .saturating_sub(inner_height)
    }
}

pub(in crate::ui) fn content_lines(app: &App) -> FrameContent {
    build_content(app, None)
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let content_area = Rect {
        width: area.width.saturating_sub(1),
        height: area.height.saturating_sub(1),
        ..area
    };
    let content = build_content(app, Some(content_area.width));
    let offset = content.scroll_offset(content_area.height);

    frame.render_widget(
        Paragraph::new(content.lines).scroll((offset, 0)),
        content_area,
    );
}

fn build_content(app: &App, width: Option<u16>) -> FrameContent {
    let health_warnings = crate::ui::overseer::health_warnings(app);
    build_content_with_warnings(app, width, &health_warnings)
}

fn build_content_with_warnings(
    app: &App,
    width: Option<u16>,
    health_warnings: &[&'static str],
) -> FrameContent {
    let selected = app.selected_item();
    let summaries =
        OverseerCategory::ALL.map(|category| crate::ui::overseer::category_summary(app, category));
    // The header is a plain label, never a selection target, so the selected
    // row only ever tracks a row below it.
    let mut lines = vec![header_line(app, health_warnings.len(), width)];
    lines.extend(warning_lines(health_warnings));
    let mut selected_row = 0;

    for (item_index, item) in app.global_escalations() {
        let alert_selected = selected == Some(Selection::OverseerAlert(item_index));
        if alert_selected {
            selected_row = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        }
        lines.push(alert_line(item, alert_selected, width));
    }

    let control_selected = selected == Some(Selection::OverseerAi);
    if control_selected {
        selected_row = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    }
    lines.push(control_ai_line(app, control_selected));

    for category in OverseerCategory::ALL {
        let category_selected = selected == Some(Selection::OverseerCategory(category));
        if category_selected {
            selected_row = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        }
        let (summary, warn) = &summaries[category.index()];
        lines.push(category_line(
            app,
            category,
            category_selected,
            summary,
            *warn,
        ));
        if !category.has_children() || !app.overseer_category_expanded(category) {
            continue;
        }
        let first_detail = lines.len();
        lines.extend(
            crate::ui::overseer::category_detail(app, category)
                .into_iter()
                .map(indent_detail),
        );
        let item_index = match selected {
            Some(Selection::DiscordChannel(index)) => Some(index),
            _ => None,
        };
        if let Some(index) = item_index {
            // A channel with a `last_error` renders two lines, so the
            // selected channel's rendered-line offset is its index plus one
            // extra line per errored channel ABOVE it — `first_detail + index`
            // alone would scroll to an earlier channel's error line instead.
            let channels = &app.overseer_snapshot.discord_channels;
            let extra_error_lines = crate::ui::overseer::ordered_channel_ids(channels)
                .iter()
                .take(index)
                .filter(|id| {
                    channels
                        .channels
                        .get(id.as_str())
                        .is_some_and(|agent| agent.last_error.is_some())
                })
                .count();
            selected_row =
                u16::try_from(first_detail + index + extra_error_lines).unwrap_or(u16::MAX);
        }
    }

    FrameContent {
        lines,
        selected_row,
    }
}

/// Plain section header, styled like the `PROJECTS` label in the tree frame:
/// no arrow and no selection marker, because it is not a focus target.
fn header_line(app: &App, warning_count: usize, width: Option<u16>) -> Line<'static> {
    let style = THEME.accent_bold_style();
    // The header never leaves the screen, so a glyph that ticks along with a
    // healthy daemon is pure noise: it draws the eye every frame without
    // carrying anything to act on. Only the error state earns a glyph here.
    // The snapshot's own `Running` / `Idle` distinction is untouched — the
    // previews and the Info pane still read it in full.
    let indicator = match app.overseer_snapshot.status() {
        Status::Dead => Some(indicator::primary_span(
            select(IndicatorState::with_status(Some(Status::Dead))),
            false,
            app.started.elapsed(),
            1,
        )),
        _ => None,
    };

    // The glyph follows the label after the same two-column gap `category_line`
    // puts between a label and its summary. Padding it out to the frame width
    // instead would strand it ~37 columns away on a wide sidebar, where it no
    // longer reads as the status of the name beside it.
    let mut spans = vec![Span::styled("OVERSEER", style)];
    if let Some(width) = width {
        // Reserve the gap and the glyph up front so a narrow frame trims the
        // label rather than the status it describes. With no glyph to place
        // there is nothing to reserve, and the label keeps those columns.
        let reserved = if indicator.is_some() {
            GAP.len() + 1
        } else {
            0
        };
        label::trim_spans_to_width(&mut spans, usize::from(width).saturating_sub(reserved));
    }
    if let Some(indicator) = indicator {
        spans.push(Span::styled(GAP, style));
        spans.push(indicator);
    }
    // Warnings trail the indicator so the glyph — when there is one — stays
    // adjacent to the label whatever the warning count is, and so they are
    // what a narrow frame drops.
    if warning_count > 0 {
        spans.push(Span::styled(
            format!("{GAP}⚠×{warning_count}"),
            warning_style(false),
        ));
    }
    if let Some(width) = width {
        label::trim_spans_to_width(&mut spans, usize::from(width));
    }
    Line::from(spans)
}

fn warning_lines<'a>(warnings: &'a [&'static str]) -> impl Iterator<Item = Line<'static>> + 'a {
    warnings
        .iter()
        .map(|warning| Line::styled(format!("⚠ {warning}"), warning_style(false)))
}

fn alert_line(
    item: &crate::ui::inbox::InboxItem,
    selected: bool,
    width: Option<u16>,
) -> Line<'static> {
    let reason = escalation_line::row_reason(&item.detail)
        .map(|reason| format!(" — {reason}"))
        .unwrap_or_default();
    let mut spans = vec![Span::styled(
        format!(
            "⚠ [{}] {} {}{reason}",
            item.kind.code(),
            item.remedy().tag(),
            item.target_id
        ),
        warning_style(selected),
    )];
    if let Some(width) = width {
        label::trim_spans_to_width(&mut spans, usize::from(width));
    }
    Line::from(spans)
}

/// The control AI row: same column layout as a category row — marker, a blank
/// arrow cell (this row never expands), the label, then a value — but the
/// value is the live status glyph rather than a summary string, since this is
/// the row an operator attaches to rather than reads.
fn control_ai_line(app: &App, selected: bool) -> Line<'static> {
    let indicator_state = IndicatorState::with_status(app.overseer_snapshot.control_status);
    let glyph =
        indicator::primary_span(select(indicator_state), selected, app.started.elapsed(), 1);
    Line::from(vec![
        // Three spaces, not the "{arrow}" `category_line` interpolates: the
        // arrow cell this row reserves is always blank, so it is spelled out
        // here instead of threading a literal space through as an argument.
        Span::styled(
            format!("{}   Control AI{GAP}", marker(selected)),
            row_style(selected),
        ),
        glyph,
    ])
}

fn category_line(
    app: &App,
    category: OverseerCategory,
    selected: bool,
    summary: &str,
    warn: bool,
) -> Line<'static> {
    // Every category row reserves the arrow cell and a leaf leaves it blank, so
    // every label lines up on one column and the arrow reads as an affordance
    // rather than as an indent. `label::agent_row_prefix` reserves the
    // management marker the same way; this follows that precedent rather than
    // inventing a second convention.
    let arrow = if !category.has_children() {
        " "
    } else if app.overseer_category_expanded(category) {
        "▾"
    } else {
        "▸"
    };
    let mut spans = vec![Span::styled(
        format!("{} {arrow} {}{GAP}", marker(selected), category.label()),
        row_style(selected),
    )];
    spans.push(Span::styled(
        summary.to_string(),
        if warn {
            warning_style(selected)
        } else if selected {
            THEME.selection_style()
        } else {
            THEME.muted_style()
        },
    ));
    Line::from(spans)
}

/// Nests the Discord channel rows directly under its label, which [`category_line`]
/// puts at column 4 — the same column every category label sits at, arrow or
/// not. The item rows carry no indent of their own, so this is the frame's
/// single indent origin for them.
fn indent_detail(line: Line<'static>) -> Line<'static> {
    let mut spans = Vec::with_capacity(line.spans.len() + 1);
    spans.push(Span::styled(DETAIL_INDENT, THEME.muted_style()));
    spans.extend(line.spans);
    Line::from(spans)
}

fn marker(selected: bool) -> &'static str {
    if selected { ">" } else { " " }
}

fn row_style(selected: bool) -> Style {
    if selected {
        THEME.selection_style()
    } else {
        THEME.accent_style()
    }
}

fn warning_style(selected: bool) -> Style {
    let style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    if selected {
        style.add_modifier(Modifier::REVERSED)
    } else {
        style
    }
}

#[cfg(test)]
mod tests;
