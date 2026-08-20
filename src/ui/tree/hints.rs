use ratatui::text::{Line, Span};

use crate::model::{OverseerCategory, Selection};

use super::super::DroprTaskFocus;
use super::THEME;

type Hints = &'static [(&'static str, &'static str)];

/// Only the keys worth a permanent slot in the footer, chosen per selected
/// row so the hints never promise a binding the row does not accept. The
/// complete keymap lives behind `?` (see `crate::ui::help`), so the footer
/// stays an entry point rather than a reference.
const NONE_HINTS: Hints = &[
    ("↵", "attach"),
    ("n", "new"),
    ("m", "merge"),
    ("?", "help"),
    ("q", "quit"),
];

const AGENT_HINTS: Hints = &[
    ("↵", "attach"),
    ("r", "restart"),
    ("m", "merge"),
    ("p", "pr"),
    ("x", "remove"),
    ("?", "help"),
    ("q", "quit"),
];

const REPO_HINTS: Hints = &[
    ("n", "new"),
    ("a", "add"),
    ("r", "reload"),
    ("?", "help"),
    ("q", "quit"),
];

const OVERSEER_AI_HINTS: Hints = &[
    ("↵", "attach"),
    ("i", "instruct"),
    ("?", "help"),
    ("q", "quit"),
];

const INBOX_CATEGORY_HINTS: Hints = &[
    ("l", "expand"),
    ("D", "clear"),
    ("?", "help"),
    ("q", "quit"),
];

/// Discord: the other expandable category, whose only footer-worthy action is
/// the expand itself.
const EXPANDABLE_CATEGORY_HINTS: Hints = &[("l", "expand"), ("?", "help"), ("q", "quit")];

const DISCORD_CHANNEL_HINTS: Hints = &[
    ("↵", "attach"),
    ("r", "retry"),
    ("x", "remove"),
    ("?", "help"),
    ("q", "quit"),
];

const OTHER_CATEGORY_HINTS: Hints = &[("?", "help"), ("q", "quit")];

const INBOX_ITEM_HINTS: Hints = &[
    ("↵", "answer"),
    ("y", "approve"),
    ("d", "dismiss"),
    ("D", "clear"),
    ("?", "help"),
    ("q", "quit"),
];

const DROPR_TASK_LIST_HINTS: Hints = &[
    ("j/k", "move"),
    ("↵", "open"),
    ("n", "start"),
    ("o", "browser"),
    ("esc", "back"),
    ("?", "help"),
    ("q", "quit"),
];

/// `Mode::TaskBody`'s hints (dropr:501). No `?`/`q`: unlike every other focus
/// level here, this mode owns input exclusively while open (see its arm in
/// `ui::input`), so those keys are genuinely inert — a hint promising a key
/// that does not work is worse than no hint.
const DROPR_TASK_BODY_HINTS: Hints = &[
    ("j/k", "scroll"),
    ("s", "start"),
    ("o", "browser"),
    ("esc", "back"),
];

const CHILD_WORKTREE_HINTS: Hints = &[("↵", "attach"), ("?", "help"), ("q", "quit")];

const ORPHAN_HINTS: Hints = &[
    ("↵", "attach"),
    ("x", "remove"),
    ("?", "help"),
    ("q", "quit"),
];

const HEADER_HINTS: Hints = &[("?", "help"), ("q", "quit")];

fn hints_for(
    selection: Option<Selection>,
    dropr_task_focus: Option<DroprTaskFocus>,
    reading_task_body: bool,
) -> Hints {
    // The drill-down (dropr:475) changes what `Selection::Repo`'s own keys
    // mean without changing the selection itself, so its hints take priority
    // over `REPO_HINTS` whenever a level is focused. `reading_task_body`
    // (`Mode::TaskBody`, dropr:501) takes priority over the list hints in
    // turn — the dialog it names is drawn over the list and owns input
    // while it is open.
    if matches!(selection, Some(Selection::Repo(_))) {
        if reading_task_body {
            return DROPR_TASK_BODY_HINTS;
        }
        if dropr_task_focus.is_some() {
            return DROPR_TASK_LIST_HINTS;
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
    reading_task_body: bool,
) -> Line<'static> {
    if let Some(text) = message {
        return Line::from(Span::styled(text.to_string(), THEME.hint_style()));
    }

    let key_hints = hints_for(selection, dropr_task_focus, reading_task_body);
    let mut spans = Vec::with_capacity(key_hints.len() * 5);
    for (key, label) in key_hints {
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
