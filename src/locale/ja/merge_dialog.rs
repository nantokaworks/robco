//! Japanese translations for `src/ui/merge_dialog.rs` — the merge
//! progress/outcome overlay and error tab.
//!
//! UI item labels (headers, field names, status chrome — e.g. the `MERGING` /
//! `MERGE COMPLETE` / `MERGE FAILED` title chrome and the `branch:` /
//! `agent:` / `repository:` field-label rows) stay English and have no entry
//! here; only content (sentences, messages, hints) is translated (dropr:377).

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        "esc dismiss" => "escで閉じる",
        _ => return None,
    })
}
