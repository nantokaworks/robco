//! Compact flag/pair-row formatting shared by [`super::render`].
//!
//! Split out of `render.rs` once that file hit the 300-line source limit —
//! see the task acceptance criteria. The row labels and enum-style values
//! these functions produce (`on`/`off`, `none`, …) are structural
//! machine-readable vocabulary, the same column-header convention
//! `src/ui/summary.rs`'s `path:` / `branch:` rows use, so none of it goes
//! through `locale::t` / `locale::fmt`.

use std::collections::BTreeMap;

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::overseer::{config::OverseerConfig, ledger::LedgerPhase};
use crate::ui::theme::DEFAULT as THEME;

pub(super) fn pair(label: &str, value: &str, warn: bool) -> Line<'static> {
    let value_style = if warn {
        Style::default().fg(Color::Red)
    } else {
        THEME.accent_style()
    };
    Line::from(vec![
        Span::styled(format!("{label}: "), THEME.muted_style()),
        Span::styled(value.to_string(), value_style),
    ])
}

pub(super) fn flags_line(segments: &[(&str, String, bool)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (label, value, warn)) in segments.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" · ", THEME.muted_style()));
        }
        spans.push(Span::styled(format!("{label}: "), THEME.muted_style()));
        let value_style = if *warn {
            Style::default().fg(Color::Red)
        } else {
            THEME.accent_style()
        };
        spans.push(Span::styled(value.clone(), value_style));
    }
    Line::from(spans)
}

pub(super) fn warning(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(Color::Red),
    ))
}

pub(super) fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

pub(super) fn list_text(items: &[String]) -> String {
    if items.is_empty() {
        "none".into()
    } else {
        items.join(", ")
    }
}

pub(super) fn map_text<K: std::fmt::Display>(map: &BTreeMap<K, usize>) -> String {
    if map.is_empty() {
        "none".into()
    } else {
        map.iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Merge recovery as one reading: the switch and, when it is on, the number of
/// handbacks a stuck pull request has left before it escalates to the operator.
pub(super) fn merge_recovery_state(config: &OverseerConfig) -> String {
    if config.merge_recovery_enabled {
        format!("on (max {})", config.max_merge_recoveries)
    } else {
        "off".into()
    }
}

pub(super) fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}
