//! Japanese translations for `src/ui/overseer/*.rs` — the Overseer panel's
//! prose.
//!
//! UI item labels (headers, field names, status chrome — e.g. `recent
//! decisions`, `what this means` / `next step` / `reason`, and the dense
//! flag/pair rows like `daemon: alive` / `dispatch: on`) stay English and
//! have no entry here; only content (sentences, messages, hints, relative
//! ages) is translated (dropr:377).
//! Row-level strings are chrome too and stay English: sidebar category
//! summary values (`{} retained`, `{}/{} actionable`, …), list placeholders
//! like `none`, and short status values fed into field rows (`worker
//! blocked`, `waiting on merge judge`) belong there, not here (dropr:388).
//! Relative ages are values, not chrome, and stay translated.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // overseer/render.rs
        "{}s ago" => "{}秒前",

        // overseer/decisions.rs
        "older entries stay in the decision log" => "それ以前の履歴はdecision logに残っています",

        // overseer/inbox_rows.rs
        "item is no longer listed" => "この項目は既に一覧から削除されています",
        "{} — no live session to answer or approve" => {
            "{} — 回答・承認できる稼働中セッションがありません"
        }

        // overseer/discord_agents.rs
        "just now" => "たった今",
        "{}m ago" => "{}分前",
        "{}h ago" => "{}時間前",
        "{}d ago" => "{}日前",
        _ => return None,
    })
}
