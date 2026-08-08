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
        "merge?" => "mergeしますか？",
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
        "enter send   esc cancel" => "enterで送信   escでキャンセル",
        "y delete   n/esc cancel" => "yで削除   n/escでキャンセル",
        "{} worker(s) {}" => "{} 個のworkerが{}",
        "y apply   n/esc cancel" => "yで適用   n/escでキャンセル",
        "y remove   n/esc cancel" => "yで削除   n/escでキャンセル",
        "y merge   n/esc cancel" => "yでmerge   n/escでキャンセル",
        "already merged: pull main, remove worktree, delete branch" => {
            "merge済み：main pull、worktree削除、ブランチ削除を実行"
        }
        "y clean up   n/esc cancel" => "yでクリーンアップ   n/escでキャンセル",
        "checking session/PR… {}" => "セッション/PR確認中… {}",
        "esc cancel" => "escでキャンセル",
        "enter send   ctrl-s save only   esc cancel" => {
            "enterで送信   ctrl-sで保存のみ   escでキャンセル"
        }
        "y delete   n/esc keep" => "yで削除   n/escで維持",
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
        "hide all {} listed item(s)" => "表示中の{}件をすべて非表示にします",
        "decisions.jsonl and ledger.json are not modified;" => {
            "decisions.jsonlとledger.jsonは変更されません；"
        }
        "a newer escalation for the same target is listed again" => {
            "同じ対象への新しいエスカレーションは再度表示されます"
        }
        "y clear   n/esc cancel" => "yで消去   n/escでキャンセル",
        _ => return None,
    })
}
