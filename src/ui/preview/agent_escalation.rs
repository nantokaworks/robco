//! DELIBERATE temporary copies of the preview body builder, `pr_label`, and
//! `field` from `ui::overseer::inbox_rows`. Leaf #585 (dropr task
//! 10mZwhUrBNq5wY64l5496) deletes `inbox_rows.rs`; keep these helpers local
//! until then rather than consolidating code around a module that is going away.

use ratatui::text::{Line, Span};

use crate::{
    locale::{fmt, t},
    model::AgentNode,
    overseer::discord::humanize,
    ui::{App, inbox::InboxItem, theme::DEFAULT as THEME},
};

const DISPLAY_ONLY: &str = "display-only";

pub(super) fn lines(app: &App, agent: &AgentNode) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (_, item) in app.escalations_for_agent(&agent.id) {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines.extend(item_lines(app, item));
    }
    lines
}

fn item_lines(app: &App, item: &InboxItem) -> Vec<Line<'static>> {
    let remedy = item.remedy();
    let mut lines = vec![
        field("kind", item.kind.label().to_string()),
        field("remedy", remedy.tag().to_string()),
        field("target", item.target_id.clone()),
        match &item.target_session {
            Some(session) => field("session", session.clone()),
            None => field(
                "session",
                fmt(
                    app.locale,
                    "{} — no live session to answer or approve",
                    &[DISPLAY_ONLY],
                ),
            ),
        },
    ];
    if let Some(url) = &item.pr_url {
        lines.push(field("pull request", pr_label(url)));
    }
    if let Some(facts) = &item.pr_facts {
        if !facts.title.is_empty() {
            lines.push(field("title", facts.title.clone()));
        }
        lines.push(field(
            "size",
            format!(
                "{} files, {} lines",
                facts.files_changed, facts.lines_changed
            ),
        ));
        if !facts.failed_checks.is_empty() {
            lines.push(field("failed check", facts.failed_checks.join(", ")));
        }
    }
    if let Some(sentence) = &item.sentence {
        lines.push(field("summary", sentence.clone()));
    }
    lines.extend([
        field("at", item.at.format("%Y-%m-%d %H:%M UTC").to_string()),
        Line::from(""),
        Line::from(Span::styled(
            t(app.locale, "what this means"),
            THEME.muted_style(),
        )),
        Line::from(Span::styled(
            t(app.locale, remedy.means),
            THEME.accent_style(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            t(app.locale, "next step"),
            THEME.muted_style(),
        )),
        Line::from(Span::styled(
            t(app.locale, remedy.next),
            THEME.accent_style(),
        )),
        Line::from(""),
        Line::from(Span::styled(t(app.locale, "reason"), THEME.muted_style())),
    ]);
    let known_sentence = humanize::static_sentence(&item.detail);
    if let Some(known) = known_sentence {
        lines.push(Line::from(Span::styled(
            t(app.locale, known),
            THEME.accent_style(),
        )));
    }
    let detail_style = if known_sentence.is_some() {
        THEME.muted_style()
    } else {
        THEME.accent_style()
    };
    lines.extend(
        item.detail
            .lines()
            .map(|line| Line::from(Span::styled(line.to_string(), detail_style))),
    );
    lines
}

fn pr_label(url: &str) -> String {
    url.rsplit_once("/pull/")
        .map(|(_, number)| format!("#{number}"))
        .unwrap_or_else(|| url.to_string())
}

fn field(name: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{name}: "), THEME.muted_style()),
        Span::styled(value, THEME.accent_style()),
    ])
}

#[cfg(test)]
#[path = "agent_escalation_tests.rs"]
mod tests;
