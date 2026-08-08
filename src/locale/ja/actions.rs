//! Japanese translations for `src/ui/actions/attach.rs`,
//! `src/ui/actions/lifecycle.rs`, `src/ui/actions/kill.rs`,
//! `src/ui/actions/pr.rs`, `src/ui/actions/discord_channels.rs`,
//! `src/ui/actions/checkout_main.rs`, and `src/ui/input/overseer.rs` — the
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

        // actions/checkout_main.rs
        "c: select a repo to check out main in its primary checkout" => {
            "c: primary checkoutでmainをcheckoutするリポジトリを選択してください"
        }
        "commit or clean untracked changes before checking out main" => {
            "mainをcheckoutする前にcommitするか未追跡の変更を整理してください"
        }
        "checked out main" => "mainをcheckoutしました",

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
        "dispatch off, workers killed; daemon still running" => {
            "dispatchを無効化しworkerを終了しました：daemonは稼働継続"
        }
        "overseer dispatch enabled" => "overseer dispatchを有効化しました",
        "overseer dispatch enabled; warning: {}" => "overseer dispatchを有効化しました。警告: {}",
        "dispatch circuit reset requested: dispatch on, failures clearing on next tick" => {
            "dispatch回路のリセットを要求しました：dispatch有効、失敗カウンタは次回tickでクリア"
        }
        "dispatch circuit reset requested: dispatch on, failures pending; warning: {}" => {
            "dispatch回路のリセットを要求しました：dispatch有効、失敗カウンタは保留中。警告: {}"
        }
        "overseer daemon is not running" => "overseerデーモンは稼働していません",
        "overseer daemon stopped" => "overseerデーモンを停止しました",
        "overseer daemon shutdown requested; a pass may still be finishing, but it will not restart automatically" => {
            "overseerデーモンの停止を要求しました：passが完了中の場合がありますが、自動再起動はしません"
        }
        "no launchd service installed; install it with `robco overseer install-service`, or run `robco overseer run` in a terminal" => {
            "launchdサービスが未インストールです。`robco overseer install-service`でインストールするか、ターミナルで`robco overseer run`を実行してください"
        }
        "launchd service management is unavailable on this OS; run `robco overseer run` in a terminal" => {
            "このOSではlaunchdサービス管理を利用できません。ターミナルで`robco overseer run`を実行してください"
        }
        "overseer is already running" => "overseerは既に稼働しています",
        "overseer started" => "overseerを起動しました",

        // actions/kill.rs
        "kill is not available for child worktrees" => "子worktreeではkillできません",
        "cannot kill an agent while it is merging" => "merge中のエージェントはkillできません",
        "remove agents first" => "先にagentを削除してください",
        "Warning: force delete discards these files:" => {
            "警告: 強制削除するとこれらのファイルは失われます:"
        }
        "... and {} more" => "…ほか{}件",
        "kill failed" => "kill失敗",
        "killed {}" => "killしました: {}",
        "cannot delete a branch while its agent is merging" => {
            "merge中のagentのブランチは削除できません"
        }
        "deleted branch {}" => "ブランチを削除しました: {}",
        "branch delete failed" => "ブランチ削除失敗",

        // actions/pr.rs
        "PR request is not available for child worktrees" => "子worktreeではPRリクエストできません",
        "select an agent to request a PR" => "PRをリクエストするagentを選択してください",
        "agent no longer exists; PR request cancelled" => {
            "agentが既に存在しません。PRリクエストを中止しました"
        }
        "PR requested: {}" => "PRをリクエストしました: {}",

        // actions/merge.rs
        "closed dialog because its agent was merged" => {
            "agentがmergeされたためダイアログを閉じました"
        }
        "merge complete: {}" => "merge完了: {}",
        "merge worker terminated unexpectedly" => "merge workerが予期せず終了しました",

        // actions/pr_precheck.rs
        "PR pre-check worker terminated unexpectedly" => {
            "PR事前チェックworkerが予期せず終了しました"
        }

        // actions/clone.rs
        "format: <git-url> [branch]" => "形式: <git-url> [branch]",
        "clone already in progress: {}" => "既にcloneが進行中です: {}",
        "cloning repository" => "リポジトリをclone中",
        "clone worker terminated unexpectedly" => "clone workerが予期せず終了しました",
        "repository added: {}" => "リポジトリを追加しました: {}",

        // actions/dropr_tasks.rs
        "a manual reload gave up before it answered; nothing was re-checked" => {
            "手動再読込が応答を待たずに終了しました。再確認は行われていません"
        }
        "the dropr task fetch panicked" => "droprタスクの取得中にpanicが発生しました",

        // actions/orphans.rs reuses "killed {}" from actions/kill.rs above.

        // actions/settings.rs
        "settings reloaded" => "設定を再読込しました",

        // input/confirm.rs
        "kept branch" => "ブランチを保持しました",

        // actions/attach.rs (dropr:371 additions)
        "channel is no longer listed" => "チャンネルは既に一覧から削除されています",

        // actions/discord_channels.rs (dropr:417)
        "reset {} to idle" => "{} をidleにリセットしました",
        "{} is not in a failed state" => "{} はfailed状態ではありません",
        "removed channel {}" => "チャンネル {} を削除しました",
        _ => return None,
    })
}
