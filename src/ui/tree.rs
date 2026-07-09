use ratatui::{
    Frame,
    layout::Alignment,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::model::{Selection, Status};

use super::{App, layout, theme::DEFAULT as THEME};

fn status_style(status: Status) -> Style {
    THEME.status_style(status)
}

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection], message: Option<&str>) {
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body);

    let mut lines = Vec::new();
    for (idx, item) in visible.iter().enumerate() {
        let selected = idx == app.selected;
        let marker = if selected { ">" } else { " " };
        let style = if selected {
            THEME.selection_style()
        } else {
            THEME.accent_style()
        };

        match *item {
            Selection::Repo(repo_idx) => {
                let repo = &app.registry.repos[repo_idx];
                let expanded = app.expanded.get(repo_idx).copied().unwrap_or(true);
                let prefix = app.config.project_icon.marker(expanded);
                let worktree_label = if repo.agents.len() == 1 {
                    "worktree"
                } else {
                    "worktrees"
                };
                let mut spans = vec![Span::styled(
                    format!(
                        "{marker} {prefix} {} ({} {worktree_label})",
                        repo.name,
                        repo.agents.len()
                    ),
                    style,
                )];
                // Off-launch-dir repos need their location spelled out — the
                // name alone no longer identifies where the repo lives.
                if !app.repo_is_local(repo) {
                    spans.push(Span::styled(
                        format!("  {}", short_path(&repo.path)),
                        if selected { style } else { THEME.muted_style() },
                    ));
                }
                // The repo's own main-worktree AI session progress. Shown only
                // when such a session is running, so the parent node reflects AI
                // work done directly on `main`.
                if let Some(status) = repo.main_status {
                    let status_text = if status == Status::Running {
                        super::spinner::frame(app.started.elapsed())
                    } else {
                        status.badge()
                    };
                    let status_style = if selected {
                        THEME.selected_status_style(status)
                    } else {
                        status_style(status)
                    };
                    spans.push(Span::styled("  ", style));
                    spans.push(Span::styled(status_text, status_style));
                }
                if repo.main_shell_working {
                    spans.push(Span::styled(" ", style));
                    spans.push(Span::styled(
                        super::spinner::term_frame(app.started.elapsed()),
                        THEME.term_style(),
                    ));
                }
                if !expanded && !repo.agents.is_empty() {
                    let status_counts = [
                        Status::Running,
                        Status::Waiting,
                        Status::Done,
                        Status::Idle,
                        Status::Dead,
                        Status::Orphaned,
                        Status::BranchOnly,
                    ]
                    .map(|status| {
                        (
                            status,
                            repo.agents
                                .iter()
                                .filter(|agent| agent.status == status)
                                .count(),
                        )
                    });
                    let status_style = |status| {
                        if selected {
                            THEME.selected_status_style(status)
                        } else {
                            status_style(status)
                        }
                    };
                    let mut first = true;
                    for (status, count) in status_counts {
                        if count == 0 {
                            continue;
                        }
                        spans.push(Span::styled(if first { "  " } else { " · " }, style));
                        spans.push(Span::styled(
                            format!("{count} {}", status.badge()),
                            status_style(status),
                        ));
                        first = false;
                    }
                }
                lines.push(Line::from(spans));
                if expanded && repo.agents.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "    (no agents)",
                        THEME.muted_style(),
                    )));
                }
            }
            Selection::Agent { repo, agent } => {
                let agent = &app.registry.repos[repo].agents[agent];
                let agent_style = if selected {
                    style
                } else if agent.status == Status::BranchOnly {
                    THEME.status_style(Status::BranchOnly)
                } else {
                    style
                };
                let status_text = if agent.status == Status::Running {
                    super::spinner::frame(app.started.elapsed())
                } else {
                    agent.status.badge()
                };
                let status_style = if selected {
                    THEME.selected_status_style(agent.status)
                } else {
                    status_style(agent.status)
                };
                let mut spans = vec![
                    Span::styled(format!("{marker}   "), style),
                    Span::styled(&agent.title, agent_style),
                    Span::raw(" "),
                    Span::styled(status_text, status_style),
                ];
                if agent.shell_working {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        super::spinner::term_frame(app.started.elapsed()),
                        THEME.term_style(),
                    ));
                }
                lines.push(Line::from(spans));
            }
            Selection::OtherHeader => {
                let count = app.other_location_repos().len();
                let arrow = if app.other_collapsed { "▸" } else { "▾" };
                let noun = if count == 1 { "repo" } else { "repos" };
                let header_style = if selected { style } else { THEME.hint_style() };
                lines.push(Line::from(Span::styled(
                    format!("{marker} {arrow} OTHER LOCATIONS ({count} {noun})"),
                    header_style,
                )));
            }
            Selection::OrphanHeader => {
                let count = app.orphans.len();
                let arrow = if app.orphans_collapsed { "▸" } else { "▾" };
                let noun = if count == 1 { "session" } else { "sessions" };
                let header_style = if selected { style } else { THEME.hint_style() };
                lines.push(Line::from(Span::styled(
                    format!("{marker} {arrow} ORPHAN SESSIONS ({count} {noun})"),
                    header_style,
                )));
            }
            Selection::Orphan(orphan_idx) => {
                let Some(orphan) = app.orphans.get(orphan_idx) else {
                    continue;
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{marker}   "), style),
                    Span::styled(orphan.name.clone(), style),
                    Span::styled(
                        format!("  {}", short_path(&orphan.cwd)),
                        if selected { style } else { THEME.muted_style() },
                    ),
                ]));
            }
        }
    }

    let tree = Paragraph::new(lines)
        .block(
            Block::default()
                .title("PROJECTS")
                .title_style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .style(THEME.accent_style());
    frame.render_widget(tree, panes.tree);

    // Status row: the ROBCO + version identity pinned bottom-left (brand
    // bold-accent, version in hint grey), key hints centred in the rest.
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let ident_width = ("ROBCO ".len() + version.chars().count() + 2) as u16;
    let zones = layout::footer_zones(root.footer, ident_width);

    let ident = Paragraph::new(Line::from(vec![
        Span::styled("ROBCO", THEME.accent_bold_style()),
        Span::styled(format!(" {version}"), THEME.hint_style()),
    ]));
    frame.render_widget(ident, zones.ident);

    let hints = Paragraph::new(hints_line(message)).alignment(Alignment::Center);
    frame.render_widget(hints, zones.hints);
}

/// Home-relative rendering of a repo path (`/Users/x/abyss/dropr` →
/// `~/abyss/dropr`), used to locate off-launch-dir repos at a glance.
fn short_path(path: &std::path::Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Key hint definitions for the footer, as `(key glyph, action label)` pairs.
/// Special keys use geeky glyphs (`⇞⇟` page, `⇥` tab, `↵` enter) to lean into
/// the ROBCO terminal aesthetic.
const KEY_HINTS: &[(&str, &str)] = &[
    ("↑↓/jk", "move"),
    ("⇞⇟", "scroll"),
    ("⇥", "pane"),
    ("↵", "attach"),
    ("n/N", "new"),
    ("r", "restart"),
    ("m", "merge"),
    ("x", "kill"),
    ("?", "help"),
    ("q", "quit"),
];

/// Build the centre key-hint line: either a transient status message, or the
/// geeky key-hint list — Norton-Commander style bracketed keys (brackets in
/// accent green, key glyph bold) with UPPERCASE labels in readable hint grey.
fn hints_line(message: Option<&str>) -> Line<'static> {
    if let Some(text) = message {
        return Line::from(Span::styled(text.to_string(), THEME.hint_style()));
    }

    let mut spans = Vec::with_capacity(KEY_HINTS.len() * 5);
    for (idx, (key, label)) in KEY_HINTS.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled("[", THEME.accent_style()));
        spans.push(Span::styled(*key, THEME.accent_bold_style()));
        spans.push(Span::styled("]", THEME.accent_style()));
        spans.push(Span::styled(
            format!(" {}", label.to_uppercase()),
            THEME.hint_style(),
        ));
    }

    Line::from(spans)
}
