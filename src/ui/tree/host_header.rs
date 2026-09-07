//! A remote host's top-level collapsible row.

use ratatui::text::{Line, Span};

use crate::{
    locale::t,
    ui::{App, actions::remote_hosts::HostConnection},
};

use super::{THEME, host_chip, label};

pub(super) fn build(
    app: &App,
    host: usize,
    selected: bool,
    marker: &str,
    width: u16,
) -> Option<Line<'static>> {
    let slot = app.hosts.get(host)?;
    let view = app.host_view(host)?;
    let repo_count = app
        .registry
        .repos
        .iter()
        .filter(|repo| repo.host.as_ref() == Some(&slot.label))
        .count();
    let has_children = match view.connection {
        HostConnection::Connecting => false,
        HostConnection::Connected => true,
        HostConnection::Failed => view.error.is_some(),
    };
    let arrow = if !has_children {
        " "
    } else if app.host_collapsed(host) {
        "▸"
    } else {
        "▾"
    };
    let (glyph, glyph_style) = host_chip::glyph(view.connection, app.started.elapsed());
    let row_style = if selected {
        THEME.selection_style()
    } else {
        THEME.accent_style()
    };
    let summary = match view.connection {
        HostConnection::Connecting => t(app.locale, "connecting...").to_string(),
        HostConnection::Connected => format!("{repo_count} repos"),
        HostConnection::Failed => view
            .error
            .as_deref()
            .and_then(|error| error.lines().next())
            .unwrap_or_default()
            .to_string(),
    };
    let summary_style = match view.connection {
        HostConnection::Failed => host_chip::failure_style(),
        _ if selected => row_style,
        _ => THEME.muted_style(),
    };
    let mut spans = vec![
        Span::styled(format!("{marker} {arrow} "), row_style),
        Span::styled(
            format!("{glyph} "),
            if selected { row_style } else { glyph_style },
        ),
        Span::styled(slot.label.name.clone(), row_style),
    ];
    if view.connection == HostConnection::Connected && !view.daemon_alive {
        spans.push(Span::styled(" ⚠", host_chip::failure_style()));
    }
    spans.push(Span::styled(format!("  {summary}"), summary_style));
    label::trim_spans_to_width(&mut spans, usize::from(width));
    Some(Line::from(spans))
}

pub(super) fn failed_child(
    app: &App,
    host: usize,
    selected: bool,
    marker: &str,
) -> Option<Line<'static>> {
    let slot = app.hosts.get(host)?;
    let view = app.host_view(host)?;
    let first_line = view.error.as_deref()?.lines().next().unwrap_or_default();
    let style = if selected {
        THEME.selection_style()
    } else {
        host_chip::failure_style()
    };
    Some(Line::from(Span::styled(
        format!("  {marker} ✗ {}: {first_line}", slot.label.name),
        style,
    )))
}
