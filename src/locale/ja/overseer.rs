//! Japanese translations for `src/ui/overseer/*.rs` — the Overseer panel's
//! prose.
//!
//! UI item labels (headers, field names, status chrome — e.g. `recent
//! decisions`, `what this means` / `next step` / `reason`, and the dense
//! flag/pair rows like `daemon: alive` / `dispatch: on`) stay English and
//! have no entry here; only content (sentences, messages, hints, relative
//! ages) is translated (dropr:377).

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // overseer/render.rs
        "{}s ago" => "{}秒前",

        // overseer/decisions.rs
        "worker blocked" => "workerがブロック中",
        "waiting on merge judge" => "merge judge待ち",
        "none" => "なし",
        "older entries stay in the decision log" => "それ以前の履歴はdecision logに残っています",

        // overseer/categories.rs
        "{}/{} actionable" => "{}/{} 対応可能",
        "{} recent" => "直近{}件",
        "no retained channels" => "保持されているチャンネルはありません",
        "{} retained" => "{}件保持中",
        "{} retained, {} failed" => "{}件保持中、{}件失敗",

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
