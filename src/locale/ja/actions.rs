//! Japanese translations for `src/ui/actions/attach.rs`,
//! `src/ui/actions/lifecycle.rs`, and `src/ui/input/overseer.rs` — the
//! status messages `show_message` renders after an operator action.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // actions/attach.rs
        "no live session in this child worktree" => {
            "この子worktreeには稼働中のセッションがありません"
        }
        "branch remains: {}" => "ブランチは残っています: {}",
        "instruction sent to overseer control" => "overseer制御に指示を送信しました",
        "reloading dropr tasks…" => "droprタスクを再読込中…",
        "failed to start dropr task reload" => "droprタスクの再読込を開始できませんでした",
        "no dropr-linked repos" => "dropr連携リポジトリがありません",
        "dropr workspaces not loaded yet" => "droprワークスペースはまだ読み込まれていません",
        "dropr workspace listing unavailable" => "droprワークスペース一覧を取得できません",
        "dropr overlay is disabled" => "dropr overlayは無効です",
        "restart is not available for child worktrees" => "子worktreeでは再起動できません",
        "cannot restart an agent while it is merging" => "merge中のエージェントは再起動できません",
        "restarted {}" => "再起動しました: {}",

        // actions/lifecycle.rs
        "repository changed, not removed" => "リポジトリが変更されたため削除しませんでした",
        "removed {}" => "削除しました: {}",
        "merge is not available for child worktrees" => "子worktreeではmergeできません",
        "merge already in progress in {}: {}" => "{}では既にmergeが進行中です: {}",
        "commit or clean untracked changes before merge" => {
            "merge前にcommitするか未追跡の変更を整理してください"
        }
        "PR for {} was closed without merging; reopen it or open a new one" => {
            "{}のPRはmergeされずcloseされました。再openするか新規PRを作成してください"
        }
        "no open PR for {}; create a PR first" => {
            "{}のopen PRがありません。先にPRを作成してください"
        }
        "path is not a git repository" => "パスがgitリポジトリではありません",
        "could not resolve path" => "パスを解決できませんでした",
        "repository already listed" => "リポジトリは既に登録済みです",
        "repository added" => "リポジトリを追加しました",

        // input/overseer.rs
        "circuit is closed; nothing to reset" => "回路はclosedです。リセットの必要はありません",
        "display-only inbox item: no live session to answer" => {
            "表示専用の受信箱項目です：回答可能な稼働中セッションがありません"
        }
        "press enter to answer the selected inbox item" => {
            "enterを押すと選択した受信箱項目に回答できます"
        }
        "inbox item is no longer listed" => "受信箱項目は既に一覧から削除されています",
        "answer sent" => "回答を送信しました",
        "approval sent" => "承認を送信しました",
        "overseer stopped: dispatch off, workers killed" => {
            "overseerを停止しました：dispatch無効化、worker終了"
        }
        "overseer dispatch enabled" => "overseer dispatchを有効化しました",
        "overseer dispatch enabled; warning: {}" => "overseer dispatchを有効化しました。警告: {}",
        "dispatch circuit reset requested: dispatch on, failures clearing on next tick" => {
            "dispatch回路のリセットを要求しました：dispatch有効、失敗カウンタは次回tickでクリア"
        }
        "dispatch circuit reset requested: dispatch on, failures pending; warning: {}" => {
            "dispatch回路のリセットを要求しました：dispatch有効、失敗カウンタは保留中。警告: {}"
        }
        _ => return None,
    })
}
