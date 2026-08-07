//! Japanese translations for `src/overseer/remedy.rs` — the fixed remedy
//! constants (`WORKER_QUESTION`, `LEDGER_PARKED`, `ORPHANED`,
//! `RECOVERABLE_FALLBACK`, `OPERATOR_FALLBACK`) whose `means` / `next`
//! sentences the Inbox preview renders under `what this means` / `next
//! step` (dropr:400). Table-driven remedies have their own translation
//! files: `remedy_merge.rs` and `remedy_runtime.rs`.
//!
//! Labels (`what this means`, `next step`, `reason`) stay English per the
//! overseer localization policy (dropr:377) and have no entry here; only
//! the `means` / `next` sentence content is translated.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // remedy::WORKER_QUESTION
        "the worker is waiting on a confirmation prompt in its own session" => {
            "workerは自分のセッション内で確認プロンプトを待っています"
        }
        "press Enter and type the answer, or `y` if it is a yes/no prompt" => {
            "Enterを押して回答を入力してください（yes/noプロンプトなら`y`）"
        }

        // remedy::LEDGER_PARKED
        "this ledger entry is parked at phase=escalated with no recorded reason, and the daemon will not retry it on its own" => {
            "このledgerエントリはphase=escalatedのまま理由が記録されず止まっており、daemonが自動で再試行することはありません"
        }
        "open the pull request directly: merge it by hand, re-dispatch the task, or clear the row with `d` once it is handled" => {
            "PRを直接開いてください：手動でmergeする、タスクを再dispatchする、または対応後に`d`で行を消去してください"
        }

        // remedy::ORPHANED
        "this needed an answer, but the worker's session is no longer live" => {
            "回答が必要でしたが、workerのセッションは既に終了しています"
        }
        "there is no session to send a fix to — handle the pull request or task by hand" => {
            "修正を送るセッションがありません — PRまたはタスクを手動で処理してください"
        }

        // remedy::RECOVERABLE_FALLBACK
        "the merge gate classified this failure as one the worker can fix" => {
            "merge gateはこの失敗をworkerが修正できるものと判定しました"
        }
        "press Enter and relay the reason to the worker, or look at the branch by hand" => {
            "Enterを押して理由をworkerに伝えるか、ブランチを手動で確認してください"
        }

        // remedy::OPERATOR_FALLBACK
        "no specific guidance is registered for this reason" => {
            "この理由に対する個別のガイダンスは登録されていません"
        }
        "review the decision log and the pull request, then decide the next step" => {
            "decision logとPRを確認し、次の対応を判断してください"
        }
        _ => return None,
    })
}
