//! Japanese translations for `src/ui/merge_dialog.rs` — the merge
//! progress/outcome overlay and error tab. The `branch:` / `agent:` /
//! `repository:` field-label rows stay structural, per the task's
//! carried-over judgment call.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        "MERGING" => "merge中",
        "MERGE COMPLETE" => "merge完了",
        "MERGE FAILED" => "merge失敗",
        "esc dismiss" => "escで閉じる",
        _ => return None,
    })
}
