//! Shared remote-host connection glyphs.

use std::time::Duration;

use ratatui::style::{Color, Modifier, Style};

use crate::ui::{actions::remote_hosts::HostConnection, spinner};

use super::THEME;

pub(super) fn glyph(connection: HostConnection, elapsed: Duration) -> (&'static str, Style) {
    match connection {
        HostConnection::Connecting => (spinner::frame(elapsed), THEME.muted_style()),
        HostConnection::Connected => ("⌁", THEME.accent_style()),
        HostConnection::Failed => ("✗", failure_style()),
    }
}

pub(super) fn failure_style() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}
