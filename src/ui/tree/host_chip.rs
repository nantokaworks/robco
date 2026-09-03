//! Remote-host state beside the PROJECTS label and its actionable detail lines.

use std::time::Duration;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::{
    locale::t,
    ui::{App, actions::remote_hosts::HostConnection, spinner, text_width::display_width},
};

use super::THEME;

const GAP: &str = "  ";

struct HostView {
    name: String,
    connection: HostConnection,
    has_repos: bool,
    daemon_alive: bool,
}

pub(super) fn lines(app: &App, width: u16, elapsed: Duration) -> Vec<Line<'static>> {
    let hosts = app
        .hosts
        .iter()
        .enumerate()
        .filter_map(|(host, slot)| {
            let view = app.host_view(host)?;
            Some(HostView {
                name: slot.label.name.clone(),
                connection: view.connection,
                daemon_alive: view.daemon_alive,
                has_repos: app
                    .registry
                    .repos
                    .iter()
                    .any(|repo| repo.host.as_ref() == Some(&slot.label)),
            })
        })
        .collect::<Vec<_>>();
    let mut lines = vec![header_line(&hosts, width, elapsed)];
    lines.extend(connecting_lines(app, &hosts, elapsed));
    lines
}

pub(super) fn failed_row(
    app: &App,
    host: usize,
    selected: bool,
    marker: &str,
) -> Option<Line<'static>> {
    let slot = app.hosts.get(host)?;
    let view = app.host_view(host)?;
    if view.connection != HostConnection::Failed {
        return None;
    }
    let first_line = view.error.as_deref()?.lines().next().unwrap_or_default();
    Some(Line::from(vec![
        Span::styled(
            format!("{marker} "),
            if selected {
                THEME.selection_style()
            } else {
                failure_style()
            },
        ),
        Span::styled(
            format!("✗ {}: {first_line}", slot.label.name),
            failure_style(),
        ),
    ]))
}

fn header_line(hosts: &[HostView], width: u16, elapsed: Duration) -> Line<'static> {
    let mut spans = vec![Span::styled("PROJECTS", THEME.accent_bold_style())];
    let mut used = display_width("PROJECTS");
    let mut dropped = false;
    for host in hosts {
        let (glyph, style) = match host.connection {
            HostConnection::Connecting => (spinner::frame(elapsed), THEME.muted_style()),
            HostConnection::Connected => ("⌁", THEME.accent_style()),
            HostConnection::Failed => ("✗", failure_style()),
        };
        let chip = format!("{GAP}{glyph} {}", host.name);
        let warning = host.connection == HostConnection::Connected && !host.daemon_alive;
        let chip_width = display_width(&chip) + usize::from(warning) * 2;
        if used + chip_width > usize::from(width) {
            dropped = true;
            break;
        }
        used += chip_width;
        spans.push(Span::styled(chip, style));
        if warning {
            spans.push(Span::styled(" ⚠", failure_style()));
        }
    }
    if dropped && used < usize::from(width) {
        spans.push(Span::styled("…", THEME.muted_style()));
    }
    Line::from(spans)
}

fn connecting_lines<'a>(
    app: &'a App,
    hosts: &'a [HostView],
    elapsed: Duration,
) -> impl Iterator<Item = Line<'static>> + 'a {
    // A connected host with zero repos has no detail row: on a wide-enough
    // pane its chip is its only representation, so clipped chips must be
    // dropped whole and announced by the header ellipsis.
    hosts
        .iter()
        .filter(|host| host.connection == HostConnection::Connecting && !host.has_repos)
        .map(move |host| {
            Line::styled(
                format!(
                    "{} {}: {}",
                    spinner::frame(elapsed),
                    host.name,
                    t(app.locale, "connecting...")
                ),
                THEME.muted_style(),
            )
        })
}

fn failure_style() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}
