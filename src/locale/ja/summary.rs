//! Japanese translations for `src/ui/summary/*.rs` — the repository summary
//! and preview panel's prose (the dense field-label rows like `path:` /
//! `branch:` stay structural, per the task's carried-over judgment call).

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // summary/dropr_tasks.rs
        "tasks unavailable" => "タスクを取得できません",
        "next tasks" => "次のタスク",
        "in progress" => "進行中",
        "blocked" => "ブロック中",
        "no open, in-progress, or blocked tasks" => "open・進行中・ブロック中のタスクはありません",
        "this list is incomplete" => "この一覧は不完全です",
        "… and at least {} more" => "…ほか少なくとも{}件",
        "… and {} more" => "…ほか{}件",

        // summary/history.rs
        "overseer has settled no tasks in this repo" => {
            "このリポジトリでoverseerが完了させたタスクはありません"
        }

        // summary.rs — agent_summary / child_summary
        "subagents" => "サブエージェント",
        "(none active or recent)" => "(稼働中・最近の実行なし)",
        "nested under agent worktree" => "agentのworktree配下にネストされています",
        _ => return None,
    })
}
