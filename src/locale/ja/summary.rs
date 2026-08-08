//! Japanese translations for `src/ui/summary/*.rs` — the repository summary
//! and preview panel's prose.
//!
//! UI item labels (headers, field names, status chrome — e.g. `next tasks` /
//! `in progress` / `blocked` / `subagents`, and the dense field-label rows
//! like `path:` / `branch:`) stay English and have no entry here; only
//! content (sentences, messages, hints) is translated (dropr:377).
//! Row-level strings are chrome too and stay English: list placeholders like
//! `(none active or recent)` and short status headings like
//! `tasks unavailable` belong there, not here (dropr:388).

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // summary/dropr_tasks.rs
        "no open, in-progress, or blocked tasks" => "open・進行中・ブロック中のタスクはありません",
        "this list is incomplete" => "この一覧は不完全です",
        "… and at least {} more" => "…ほか少なくとも{}件",
        "… and {} more" => "…ほか{}件",

        // summary/history.rs
        "overseer has settled no tasks in this repo" => {
            "このリポジトリでoverseerが完了させたタスクはありません"
        }

        // summary.rs — child_summary
        "nested under agent worktree" => "agentのworktree配下にネストされています",

        // summary.rs — checkout_branch_warning
        "HEAD is detached — press c to check out main (clean tree only)" => {
            "HEADがdetached状態です — cキーでmainをcheckoutできます（作業ツリーがcleanな場合のみ）"
        }
        "on branch {}, not main — press c to check out main (clean tree only)" => {
            "mainではなくブランチ{}上です — cキーでmainをcheckoutできます（作業ツリーがcleanな場合のみ）"
        }

        // summary.rs — main_drift_warning
        "main behind origin/main by {}" => "mainがorigin/mainより{}コミット遅れています",
        _ => return None,
    })
}
