//! Japanese translations for `src/overseer/remedy/table/runtime.rs` — the
//! dispatch / triage / review remedy table. The Inbox preview routes every
//! `Remedy` it resolves through `locale::t`, so this table's `means` /
//! `next` sentences need a matching entry here (dropr:400).
//!
//! Labels (`what this means`, `next step`, `reason`) stay English per the
//! overseer localization policy (dropr:377) and have no entry here; only
//! the `means` / `next` sentence content is translated. `worker_blocked`
//! and `worker blocked` (two distinct daemon-side reason codes) share one
//! English sentence and therefore one arm here.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // table::runtime::EXACT — "dispatch disabled pending operator reset"
        "the dispatch circuit tripped after repeated consecutive failures, so dispatch is off" => {
            "連続した失敗によりdispatch回路がtripし、dispatchは無効になっています"
        }
        "fix the underlying failure, then press `R` to reset the circuit" => {
            "根本原因を修正し、`R`を押して回路をリセットしてください"
        }

        // table::runtime::EXACT — "worker_blocked" / "worker blocked"
        "the worker reported it is blocked" => "workerがblockedと報告しました",
        "press Enter and tell it how to proceed" => "Enterを押して、進め方を伝えてください",

        // table::runtime::EXACT — "triage session timed out"
        "the triage session did not respond within its timeout" => {
            "triageセッションがtimeout内に応答しませんでした"
        }
        "check the triage backend is reachable, then re-run triage on the task" => {
            "triageのbackendに到達できるか確認し、タスクのtriageを再実行してください"
        }

        // table::runtime::EXACT — "triage session exited without result.json"
        "the triage session exited without writing a result" => {
            "triageセッションが結果を書き込まずに終了しました"
        }
        "check the triage session log, then re-run triage on the task" => {
            "triageセッションのログを確認し、タスクのtriageを再実行してください"
        }

        // table::runtime::PREFIX — "spawn_failed:"
        "the daemon failed to spawn a worker for this candidate" => {
            "daemonがこの候補のworker起動に失敗しました"
        }
        "check the spawn error and the repository's worktree state, then let the next dispatch pass retry it" => {
            "起動エラーとリポジトリのworktree状態を確認し、次のdispatch passで再試行させてください"
        }

        // table::runtime::PREFIX — "circuit_open:"
        "the failure circuit is open; dispatch has stopped" => {
            "失敗回路がopenです。dispatchは停止しています"
        }

        // table::runtime::PREFIX — "circuit_at_risk:"
        "consecutive failures are approaching the circuit threshold" => {
            "連続した失敗が回路のしきい値に近づいています"
        }
        "nothing to do yet unless it trips" => "tripしない限り対応不要です",

        // table::runtime::PREFIX — "triage action failed:"
        "a triage action ran and failed" => "triage actionが実行され、失敗しました",
        "check the triage backend logs, then re-run triage on the task" => {
            "triageのbackendログを確認し、タスクのtriageを再実行してください"
        }

        // table::runtime::PREFIX — "triage session failed:"
        "the triage session failed to launch" => "triageセッションの起動に失敗しました",
        "check the triage backend, then re-run triage on the task" => {
            "triageのbackendを確認し、タスクのtriageを再実行してください"
        }

        // table::runtime::PREFIX — "malformed result.json:"
        "the session's result did not parse" => "セッションの結果をparseできませんでした",
        "check the session log for what it actually wrote" => {
            "セッションログを確認し、実際に何が書き込まれたか確認してください"
        }

        // table::runtime::PREFIX — "session_auth_failed:"
        "the session could not authenticate" => "セッションが認証できませんでした",
        "check the credentials for the worker, triage, or review backend, then retry" => {
            "worker、triage、reviewのbackendの認証情報を確認し、再試行してください"
        }
        _ => return None,
    })
}
