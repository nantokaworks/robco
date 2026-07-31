use ratatui::text::{Line, Span};

use crate::model::{OverseerCategory, Selection};

use super::THEME;

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

/// Discord and Details: expandable categories whose only footer-worthy action
/// is the expand itself.
const EXPANDABLE_CATEGORY_HINTS: Hints = &[
    ("l", "expand"),
    ("R", "reset"),
    ("?", "help"),
    ("q", "quit"),
];

const DISCORD_CHANNEL_HINTS: Hints = &[
    ("↵", "attach"),
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

fn hints_for(selection: Option<Selection>) -> Hints {
    match selection {
        None => NONE_HINTS,
        Some(Selection::Agent { .. }) => AGENT_HINTS,
        Some(Selection::Repo(_)) => REPO_HINTS,
        Some(Selection::OverseerAi) => OVERSEER_AI_HINTS,
        Some(Selection::OverseerCategory(OverseerCategory::Inbox)) => INBOX_CATEGORY_HINTS,
        Some(Selection::OverseerCategory(
            OverseerCategory::Discord | OverseerCategory::Details,
        )) => EXPANDABLE_CATEGORY_HINTS,
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
    circuit_open: bool,
) -> Line<'static> {
    if let Some(text) = message {
        return Line::from(Span::styled(text.to_string(), THEME.hint_style()));
    }

    let key_hints = hints_for(selection);
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
mod tests {
    use super::*;

    #[test]
    fn reset_hint_is_only_shown_when_circuit_is_open() {
        assert!(
            hints_line(None, None, true)
                .to_string()
                .contains("[R] RESET")
        );
        assert!(
            !hints_line(None, None, false)
                .to_string()
                .contains("[R] RESET")
        );
    }

    #[test]
    fn footer_carries_only_the_essential_hints() {
        let line = hints_line(None, None, false).to_string();
        assert_eq!(line, "[↵] ATTACH [n] NEW [m] MERGE [?] HELP [q] QUIT");
    }

    #[test]
    fn agent_row_advertises_its_own_actions() {
        let line =
            hints_line(None, Some(Selection::Agent { repo: 0, agent: 0 }), false).to_string();
        assert_eq!(
            line,
            "[↵] ATTACH [r] RESTART [m] MERGE [p] PR [g] MANAGE [x] REMOVE [?] HELP [q] QUIT"
        );
    }

    #[test]
    fn repo_row_advertises_its_own_actions() {
        let line = hints_line(None, Some(Selection::Repo(0)), false).to_string();
        assert_eq!(
            line,
            "[n] NEW [a] ADD [r] RELOAD [g] MANAGE [?] HELP [q] QUIT"
        );
    }

    #[test]
    fn overseer_ai_row_advertises_attach_and_instruct() {
        let line = hints_line(None, Some(Selection::OverseerAi), false).to_string();
        assert_eq!(line, "[↵] ATTACH [i] INSTRUCT [?] HELP [q] QUIT");
    }

    #[test]
    fn inbox_category_advertises_expand_and_clear() {
        let line = hints_line(
            None,
            Some(Selection::OverseerCategory(OverseerCategory::Inbox)),
            false,
        )
        .to_string();
        assert_eq!(line, "[l] EXPAND [D] CLEAR [?] HELP [q] QUIT");
    }

    #[test]
    fn other_overseer_categories_carry_no_extra_action() {
        let line = hints_line(
            None,
            Some(Selection::OverseerCategory(OverseerCategory::Health)),
            false,
        )
        .to_string();
        assert_eq!(line, "[?] HELP [q] QUIT");
    }

    #[test]
    fn inbox_item_advertises_answer_approve_dismiss_clear() {
        let line = hints_line(None, Some(Selection::OverseerInbox(0)), false).to_string();
        assert_eq!(
            line,
            "[↵] ANSWER [y] APPROVE [d] DISMISS [D] CLEAR [?] HELP [q] QUIT"
        );
    }

    #[test]
    fn child_worktree_advertises_attach_only() {
        let line = hints_line(
            None,
            Some(Selection::ChildWorktree {
                repo: 0,
                agent: 0,
                child: 0,
            }),
            false,
        )
        .to_string();
        assert_eq!(line, "[↵] ATTACH [?] HELP [q] QUIT");
    }

    #[test]
    fn orphan_advertises_attach_and_remove() {
        let line = hints_line(None, Some(Selection::Orphan(0)), false).to_string();
        assert_eq!(line, "[↵] ATTACH [x] REMOVE [?] HELP [q] QUIT");
    }

    #[test]
    fn section_headers_carry_no_extra_action() {
        let line = hints_line(None, Some(Selection::OtherHeader), false).to_string();
        assert_eq!(line, "[?] HELP [q] QUIT");
        let line = hints_line(None, Some(Selection::OrphanHeader), false).to_string();
        assert_eq!(line, "[?] HELP [q] QUIT");
    }
}
