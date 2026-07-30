//! Japanese translations for `src/ui/input.rs` and `src/ui/input/*.rs` — the
//! keybinding dispatch layer's status messages.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // input/inbox_dismiss.rs
        "dismissed [{}] {}" => "却下しました [{}] {}",
        "inbox is already empty" => "受信箱は既に空です",
        "dismissed {} inbox item(s)" => "受信箱の{}件を却下しました",

        // input.rs
        "created agent {}" => "agentを作成しました: {}",
        "created agent {}, but its repository is no longer registered" => {
            "agentを作成しましたが、そのリポジトリは既に登録から外れています: {}"
        }
        "saved PR prompt to config" => "PRプロンプトをconfigに保存しました",
        "merge in progress: {} — wait or ctrl-c to force quit" => {
            "merge進行中: {} — 待つか、ctrl-cで強制終了してください"
        }
        "dismissed merge notice" => "merge通知を閉じました",
        "IME is on; switch to ASCII input" => "IMEが有効です。ASCII入力に切り替えてください",

        // input/management.rs
        "g: select a repo or a worktree to cycle overseer management" => {
            "g: overseer管理を切り替えるリポジトリかworktreeを選択してください"
        }
        "overseer management: auto" => "overseer管理: auto",
        "overseer management: manual" => "overseer管理: manual",
        "detached from overseer management (worker left running)" => {
            "overseer管理から切り離しました(workerは起動したまま)"
        }
        "g: this worktree belongs to another agent, not the overseer" => {
            "g: このworktreeはoverseerではなく別のagentに属しています"
        }
        "g: selected worktree was not found" => "g: 選択したworktreeが見つかりませんでした",
        "g: no worktrees under this repo for the overseer to manage" => {
            "g: このリポジトリにはoverseerが管理できるworktreeがありません"
        }
        "g: no workers left to {}" => "g: {} 対象のworkerがもうありません",
        "1 worker {}" => "workerを1台 {}",
        "{} workers {}" => "workerを{}台 {}",
        "G: select a repo to toggle overseer management" => {
            "G: overseer管理を切り替えるリポジトリを選択してください"
        }
        "overseer management: repo back under auto" => "overseer管理: リポジトリをautoに戻しました",
        "overseer management: repo opted out (running workers keep going)" => {
            "overseer管理: リポジトリを対象外にしました(実行中のworkerはそのまま継続)"
        }
        _ => return None,
    })
}
