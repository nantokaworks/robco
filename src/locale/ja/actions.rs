//! Japanese translations for `src/ui/actions/attach.rs`,
//! `src/ui/actions/lifecycle.rs`, `src/ui/actions/kill.rs`,
//! `src/ui/actions/pr.rs`, `src/ui/actions/discord_channels.rs`,
//! `src/ui/actions/checkout_main.rs`, `src/ui/actions/dropr_task_drill.rs`,
//! `src/ui/actions/dropr_task_open.rs`, and `src/ui/input/overseer.rs` —
//! the status messages `show_message` renders after an operator action.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // actions/attach.rs
        "no live session in this child worktree" => {
            "この子worktreeには稼働中のセッションがありません"
        }
        "branch remains: {}" => "ブランチは残っています: {}",
        "instruction sent to overseer control" => "overseer制御に指示を送信しました",
        "instruction sent" => "指示を送信しました",
        "reloading dropr tasks…" => "droprタスクを再読込中…",
        "failed to start dropr task reload" => "droprタスクの再読込を開始できませんでした",
        "no dropr-linked repos" => "dropr連携リポジトリがありません",
        "dropr workspaces not loaded yet" => "droprワークスペースはまだ読み込まれていません",
        "dropr workspace listing unavailable" => "droprワークスペース一覧を取得できません",
        "dropr overlay is disabled" => "dropr overlayは無効です",
        "linked repos have no materialised dropr board yet" => {
            "連携リポジトリにmaterializeされたdroprボードがまだありません"
        }
        "restart is not available for child worktrees" => "子worktreeでは再起動できません",
        "cannot restart an agent while it is merging" => "merge中のエージェントは再起動できません",
        "restarted {}" => "再起動しました: {}",

        // actions/dropr_task_drill.rs
        "task is no longer listed" => "タスクは一覧にありません",
        "no dropr workspace linked to this repo" => {
            "このリポジトリに紐づくdroprワークスペースがありません"
        }
        "task is missing its dropr id" => "タスクにdropr id(nanoid)がありません",
        "{} already has a live worker: {}" => "{} には稼働中のworkerが既にあります: {}",
        "{} already has a branch: {}" => "{} には既存のブランチがあります: {}",
        "could not confirm {}'s subtasks — refresh the task list and try again" => {
            "{} のサブタスクを確認できませんでした。タスク一覧を再読込してから、もう一度お試しください"
        }
        "could not claim {}: {}" => "{} をclaimできませんでした: {}",
        "could not reach dropr to claim {}" => "{} のclaimでdroprに到達できませんでした",
        "launched {} for {}" => "{} を {} 向けに起動しました",
        "launched {}, but its repository is no longer registered" => {
            "{} を起動しましたが、そのリポジトリは登録が解除されています"
        }
        "a launch is already in progress: {}" => "起動処理が既に進行中です: {}",
        "launching {}…" => "{} を起動中…",
        "could not launch {}: {}" => "{} を起動できませんでした: {}",
        "launched {}, but could not save the registry: {}" => {
            "{} を起動しましたが、レジストリを保存できませんでした: {}"
        }
        "launch worker for {} terminated unexpectedly" => "{} の起動ワーカーが予期せず終了しました",

        // actions/dropr_task_open.rs
        "this repository has no git remote" => "このリポジトリにはgit remoteがありません",
        "could not build a console URL for this repository" => {
            "このリポジトリのコンソールURLを作成できませんでした"
        }
        "opened {} in the browser" => "{} をブラウザで開きました",
        "copied the URL for {}: {}" => "{} のURLをコピーしました: {}",
        "could not open {}: {}" => "{} を開けませんでした: {}",

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

        // actions/rename.rs
        "repository changed, not renamed" => "リポジトリが変更されたため名前を変更しませんでした",
        "renamed to {}" => "名前を変更しました: {}",
        "renamed the directory to {}, but it was no longer in robco's registry" => {
            "ディレクトリを {} に変更しましたが、robcoの登録情報からは既に消えていました"
        }
        "renamed the directory to {}, but could not update robco's saved state ({}); restart robco to re-discover it" => {
            "ディレクトリを {} に変更しましたが、robcoの保存状態を更新できませんでした（{}）。robcoを再起動して再検出してください"
        }
        "rename incomplete" => "名前変更が未完了です",
        "these worktrees still need manual repair:" => "次のworktreeは手動での修復が必要です：",
        "run: git -C {} worktree repair <worktree-path>" => {
            "実行してください: git -C {} worktree repair <worktree-path>"
        }

        // actions/checkout_main.rs
        "c: select a repo to check out its default branch in its primary checkout" => {
            "c: primary checkoutでdefault branchをcheckoutするリポジトリを選択してください"
        }
        "commit or clean untracked changes before checking out {}" => {
            "{}をcheckoutする前にcommitするか未追跡の変更を整理してください"
        }
        "checked out {}" => "{}をcheckoutしました",
        "default branch could not be resolved — run git remote set-head origin -a" => {
            "default branchを特定できませんでした — git remote set-head origin -aを実行してください"
        }

        // actions/clear_chat.rs
        "C: select a repo to clear its chat session" => {
            "C: チャットセッションをクリアするリポジトリを選択してください"
        }
        "no clear command configured for {}" => "{}向けのclearコマンドが未設定です",
        "no live chat session to clear" => "クリア対象の稼働中チャットセッションがありません",
        "tmux is not installed, or not on PATH" => {
            "tmuxがインストールされていないか、PATHに存在しません"
        }
        "chat session is busy — wait for it to finish before clearing" => {
            "チャットセッションはbusyです。クリアする前に完了を待ってください"
        }
        "cleared chat session for {}" => "{}のチャットセッションをクリアしました",
        "repository changed, not cleared" => "リポジトリが変更されたためクリアしませんでした",

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
        "no launchd service installed; install it with `robco service install`, or run `robco daemon` in a terminal" => {
            "launchdサービスが未インストールです。`robco service install`でインストールするか、ターミナルで`robco daemon`を実行してください"
        }
        "launchd service management is unavailable on this OS; run `robco daemon` in a terminal" => {
            "このOSではlaunchdサービス管理を利用できません。ターミナルで`robco daemon`を実行してください"
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
        "Nothing was merged because these checks failed: {}" => {
            "次のチェックが失敗したためmergeしませんでした: {}"
        }
        "an unnamed check" => "名前のないチェック",
        "Approval queued; it will merge once the checks pass" => {
            "承認をキューに追加しました。チェック通過後にmergeします"
        }
        "PR requested and approval queued; it will merge once the checks pass" => {
            "PRをリクエストし、承認をキューに追加しました。チェック通過後にmergeします"
        }
        "approval queued; waiting for the merge gate" => "承認済みです。mergeゲートを待っています",

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
