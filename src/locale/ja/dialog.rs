//! Japanese translations for `src/ui/dialog/content.rs` — dialog bodies and
//! confirmation hints.
//!
//! UI item labels (headers, field names, status chrome — e.g. the input
//! field labels like `agent` / `prompt` and the noun-phrase titles like
//! `new agent` / `add repo` / `help`) stay English and have no entry here;
//! only content (sentences, messages, hints) is translated (dropr:377).
//! Question-form confirmation titles are full sentences, so they keep their
//! translations.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // Titles (question-form confirmations only)
        "delete worktree?" => "worktreeを削除しますか？",
        "manage whole repo?" => "リポジトリ全体を管理しますか？",
        "remove repo?" => "リポジトリを削除しますか？",
        "land task?" => "タスクをlandしますか？",
        "clean up merged PR?" => "merge済みPRをクリーンアップしますか？",
        "request PR from agent?" => "エージェントにPRを依頼しますか？",
        "delete branch?" => "ブランチを削除しますか？",
        "kill session?" => "セッションを終了しますか？",
        "stop overseer?" => "overseerを停止しますか？",
        "reset dispatch circuit?" => "dispatch回路をリセットしますか？",
        "stop the overseer daemon?" => "overseerデーモンを停止しますか？",
        "clear the overseer inbox?" => "overseer受信箱を消去しますか？",
        "remove channel?" => "チャンネルを削除しますか？",

        // Body / hint text
        "Create a new agent with an optional | initial prompt." => {
            "新規エージェントを作成します（| 区切りで初期プロンプトを指定可）。"
        }
        "format: title | initial prompt" => "形式: タイトル | 初期プロンプト",
        "enter create   esc cancel" => "enterで作成   escでキャンセル",
        "git format: <git-url> [branch]" => "形式: <git-url> [branch]",
        "enter add   esc cancel" => "enterで追加   escでキャンセル",
        "enter rename   esc cancel" => "enterで名前変更   escでキャンセル",
        "enter send   esc cancel" => "enterで送信   escでキャンセル",
        "y delete   n/esc cancel" => "yで削除   n/escでキャンセル",
        "ends the running session now and removes the worktree" => {
            "実行中のセッションを直ちに終了し、worktreeを削除します"
        }
        "{} worker(s) {}" => "{} 個のworkerが{}",
        "y apply   n/esc cancel" => "yで適用   n/escでキャンセル",
        "y remove   n/esc cancel" => "yで削除   n/escでキャンセル",
        "It will merge now" => "今すぐmergeします",
        "Approval is queued; it will merge once the checks pass" => {
            "承認はキューに追加済みです。チェック通過後にmergeします"
        }
        "It will open a pull request and queue approval; it will merge once the checks pass" => {
            "PRを作成して承認をキューに追加します。チェック通過後にmergeします"
        }
        "y land   n/esc cancel" => "yでland   n/escでキャンセル",
        "already merged" => "merge済みです",
        "pulls main, removes the worktree, deletes the branch here and \
     on GitHub, and ends the running session — anything left uncommitted in \
     the worktree is discarded" => {
            "main をpullし、worktreeを削除し、ブランチをローカルとGitHubの両方から削除し、\
             実行中のセッションを終了します。worktreeにコミットされていない変更が残っていれば、\
             それも破棄されます"
        }
        "y clean up   n/esc cancel" => "yでクリーンアップ   n/escでキャンセル",
        "checking session/PR… {}" => "セッション/PR確認中… {}",
        "esc cancel" => "escでキャンセル",
        "enter send   ctrl-s save only   esc cancel" => {
            "enterで送信   ctrl-sで保存のみ   escでキャンセル"
        }
        "y delete   n/esc keep" => "yで削除   n/escで維持",
        "force delete: any commits not merged elsewhere are lost" => {
            "強制削除：他にmergeされていないコミットは失われます"
        }
        "y kill   n/esc cancel" => "yで終了   n/escでキャンセル",
        "disable dispatch + kill all overseer workers" => {
            "dispatchを無効化し全overseer workerを終了"
        }
        "daemon stays alive; press S again to turn dispatch back on" => {
            "daemonは稼働継続。Sを再度押すとdispatchを再開"
        }
        "y stop   n/esc cancel" => "yで停止   n/escでキャンセル",
        "re-enable dispatch and clear the failure counter" => {
            "dispatchを再有効化し失敗カウンタをクリア"
        }
        "y reset   n/esc cancel" => "yでリセット   n/escでキャンセル",
        "ends the daemon process itself, not just dispatch" => {
            "dispatchだけでなくdaemonプロセス自体を終了します"
        }
        "running workers are not touched; start it again with R" => {
            "稼働中のworkerには影響しません。Rで再度起動できます"
        }
        "hiding {} item(s):" => "{}件を非表示にします：",
        "nothing on record is deleted, only removed from this list" => {
            "記録は削除されません。このリストから消えるだけです"
        }
        "a hidden item returns only if the same target escalates again; \
                     otherwise it stays hidden even if it still needs you" => {
            "非表示にした項目は、同じ対象が再度エスカレーションした時だけ再表示されます。\
             対応が必要なままでも、それ以外は非表示のままです"
        }
        "y clear   n/esc cancel" => "yで消去   n/escでキャンセル",
        "deletes its whole record, history included — this cannot be undone" => {
            "履歴を含む記録全体を削除します。元に戻せません"
        }
        _ => return None,
    })
}
