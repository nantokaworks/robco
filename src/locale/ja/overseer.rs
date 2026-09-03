//! Japanese translations for `src/ui/overseer/*.rs` — the Overseer panel's
//! prose.
//!
//! UI item labels and status chrome stay English. Only content such as
//! sentences, messages, hints, and relative ages is translated (dropr:377).
//! Relative ages are values, not chrome, and stay translated.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // overseer/discord_agents.rs
        "just now" => "たった今",
        "{}m ago" => "{}分前",
        "{}h ago" => "{}時間前",
        "{}d ago" => "{}日前",
        _ => return None,
    })
}
