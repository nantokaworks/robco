use ratatui::text::{Line, Span};

use crate::model::{OverseerCategory, Selection};

use super::THEME;
use super::super::DroprTaskFocus;

type Hints = &'static [(&'static str, &'static str)];

/// Only the keys worth a permanent slot in the footer, chosen per selected
/// row so the hints never promise a binding the row does not accept. The
/// complete keymap lives behind `?` (see `crate::ui::help`), so the footer
/// stays an entry point rather than a reference. `R` (dispatch circuit reset)
/// is folded into every list but only rendered while the circuit is open —
/// see `hints_line`.
const NONE_HINTS: Hints = &[
    ("↵", "attach"),
    ("n", "new"),
    ("m", "merge"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const AGENT_HINTS: Hints = &[
    ("↵", "attach"),
    ("r", "restart"),
    ("m", "merge"),
    ("p", "pr"),
    ("g", "manage"),
    ("x", "remove"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const REPO_HINTS: Hints = &[
    ("n", "new"),
    ("a", "add"),
    ("r", "reload"),
    ("g", "manage"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const OVERSEER_AI_HINTS: Hints = &[
    ("↵", "attach"),
    ("i", "instruct"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const INBOX_CATEGORY_HINTS: Hints = &[
    ("l", "expand"),
    ("D", "clear"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

/// Discord: the other expandable category, whose only footer-worthy action is
/// the expand itself.
const EXPANDABLE_CATEGORY_HINTS: Hints = &[
    ("l", "expand"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const DISCORD_CHANNEL_HINTS: Hints = &[
    ("↵", "attach"),
    ("r", "retry"),
    ("x", "remove"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const OTHER_CATEGORY_HINTS: Hints = &[("R", "reset"), ("?", "help"), ("q", "quit")];

const INBOX_ITEM_HINTS: Hints = &[
    ("↵", "answer"),
    ("y", "approve"),
    ("d", "dismiss"),
    ("D", "clear"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const DROPR_TASK_LIST_HINTS: Hints = &[
    ("j/k", "move"),
    ("↵", "open"),
    ("esc", "back"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const DROPR_TASK_BODY_HINTS: Hints = &[
    ("s", "start"),
    ("esc", "back"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const CHILD_WORKTREE_HINTS: Hints = &[
    ("↵", "attach"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const ORPHAN_HINTS: Hints = &[
    ("↵", "attach"),
    ("x", "remove"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const HEADER_HINTS: Hints = &[("R", "reset"), ("?", "help"), ("q", "quit")];

fn hints_for(selection: Option<Selection>, dropr_task_focus: Option<DroprTaskFocus>) -> Hints {
    // The drill-down (dropr:475) changes what `Selection::Repo`'s own keys
    // mean without changing the selection itself, so its hints take priority
    // over `REPO_HINTS` whenever a level is focused.
    if matches!(selection, Some(Selection::Repo(_))) {
        match dropr_task_focus {
            Some(DroprTaskFocus::List { .. }) => return DROPR_TASK_LIST_HINTS,
            Some(DroprTaskFocus::Body { .. }) => return DROPR_TASK_BODY_HINTS,
            None => {}
        }
    }
    match selection {
        None => NONE_HINTS,
        Some(Selection::Agent { .. }) => AGENT_HINTS,
        Some(Selection::Repo(_)) => REPO_HINTS,
        Some(Selection::OverseerAi) => OVERSEER_AI_HINTS,
        Some(Selection::OverseerCategory(OverseerCategory::Inbox)) => INBOX_CATEGORY_HINTS,
        Some(Selection::OverseerCategory(OverseerCategory::Discord)) => EXPANDABLE_CATEGORY_HINTS,
        Some(Selection::OverseerCategory(_)) => OTHER_CATEGORY_HINTS,
        Some(Selection::OverseerInbox(_)) => INBOX_ITEM_HINTS,
        Some(Selection::DiscordChannel(_)) => DISCORD_CHANNEL_HINTS,
        Some(Selection::ChildWorktree { .. }) => CHILD_WORKTREE_HINTS,
        Some(Selection::Orphan(_)) => ORPHAN_HINTS,
        Some(Selection::OtherHeader) | Some(Selection::OrphanHeader) => HEADER_HINTS,
    }
}

pub(super) fn hints_line(
    message: Option<&str>,
    selection: Option<Selection>,
    dropr_task_focus: Option<DroprTaskFocus>,
    circuit_open: bool,
) -> Line<'static> {
    if let Some(text) = message {
        return Line::from(Span::styled(text.to_string(), THEME.hint_style()));
    }

    let key_hints = hints_for(selection, dropr_task_focus);
    let mut spans = Vec::with_capacity(key_hints.len() * 5);
    for (key, label) in key_hints {
        if *key == "R" && !circuit_open {
            continue;
        }
        if !spans.is_empty() {
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

#[cfg(test)]
#[path = "hints_tests.rs"]
mod tests;
