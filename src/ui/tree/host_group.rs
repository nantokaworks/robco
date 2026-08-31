use ratatui::text::{Line, Span};

use crate::{locale::fmt, model::HostLabel};

use super::{App, THEME};

pub(super) fn before_repo(
    app: &App,
    repo: usize,
    shown: &mut Vec<HostLabel>,
    lines: &mut Vec<Line<'static>>,
) {
    let Some(host) = app.registry.repos[repo].host.as_ref() else {
        return;
    };
    if shown.contains(host) {
        return;
    }
    push_group(app, host, lines);
    shown.push(host.clone());
}

pub(super) fn finish(app: &App, shown: &mut Vec<HostLabel>, lines: &mut Vec<Line<'static>>) {
    for slot in &app.hosts {
        if !shown.contains(&slot.label) {
            push_group(app, &slot.label, lines);
            shown.push(slot.label.clone());
        }
    }
}

fn push_group(app: &App, host: &HostLabel, lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(Span::styled(
        format!("  HOST {}", host.name),
        THEME.accent_bold_style(),
    )));
    if let Some(error) = app
        .hosts
        .iter()
        .find(|slot| slot.label == *host)
        .and_then(|slot| slot.error())
    {
        lines.push(Line::from(Span::styled(
            format!(
                "    {}",
                fmt(app.locale, "remote host error: {}", &[&error])
            ),
            THEME.failure_style(),
        )));
    }
}

pub(super) fn short_path(path: &std::path::Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
