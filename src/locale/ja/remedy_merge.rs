//! Japanese translations for `src/overseer/remedy/table/merge.rs` — the
//! auto-merge gate's remedy table. The Inbox preview routes every `Remedy`
//! it resolves through `locale::t`, so this table's `means` / `next`
//! sentences need a matching entry here (dropr:400).
//!
//! Labels (`what this means`, `next step`, `reason`) stay English per the
//! overseer localization policy (dropr:377) and have no entry here; only
//! the `means` / `next` sentence content is translated. Some `next`
//! sentences repeat verbatim across table entries; one arm covers every
//! call site that produces the same English text.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // table::merge::EXACT — "autonomy_envelope"
        "the autonomy envelope held this back for an operator's own decision" => {
            "autonomy envelopeが、operator自身の判断のためにこれを保留しました"
        }
        "merge it by hand (`m` on the agent row), or raise it with `robco overseer autonomy <level>`" => {
            "手動でmergeする（agent行で`m`）か、`robco overseer autonomy <level>`でautonomyレベルを引き上げてください"
        }

        // table::merge::EXACT — "checks_waiting"
        "the pull request's checks are still running; nothing has failed" => {
            "PRのchecksはまだ実行中です。失敗はありません"
        }
        "nothing to do until a check finishes" => "checkが完了するまで対応不要です",

        // table::merge::EXACT — merge_queue::WAITING_TURN
        "this pull request is behind its base, but another pull request in the same repository is ahead of it in the merge queue" => {
            "このPRはbaseより遅れていますが、同じリポジトリの別PRがmerge queueで先行しています"
        }
        "nothing to do until the pull request ahead of it merges" => {
            "先行するPRがmergeされるまで対応不要です"
        }

        // table::merge::EXACT — "checks_not_green"
        "a required check finished and failed on the worker's own head" => {
            "workerのheadで必須checkが完了し、失敗しました"
        }
        "press Enter and tell the worker to fix the failing check and push" => {
            "Enterを押して、失敗しているcheckを修正してpushするようworkerに伝えてください"
        }

        // table::merge::EXACT — "merge_state:dirty"
        "the branch conflicts with its base" => "ブランチがbaseと競合しています",
        "press Enter and tell the worker to rebase onto the base branch and push" => {
            "Enterを押して、baseブランチにrebaseしてpushするようworkerに伝えてください"
        }

        // table::merge::EXACT — "merge_state:blocked"
        "the branch is missing a required review or a required check" => {
            "ブランチに必須のreviewまたはcheckが不足しています"
        }
        "press Enter and tell the worker to satisfy the missing review or check and push" => {
            "Enterを押して、不足しているreviewまたはcheckを満たしてpushするようworkerに伝えてください"
        }

        // table::merge::EXACT — merge_state::UPDATE_CAP_REACHED
        "the automated budget for keeping this branch caught up with its base is spent" => {
            "このブランチをbaseに追従させ続けるための自動予算を使い切りました"
        }
        "press Enter and tell the worker to rebase onto the base branch itself" => {
            "Enterを押して、baseブランチへ自分でrebaseするようworkerに伝えてください"
        }

        // table::merge::EXACT — "missing_pr_url"
        "the ledger has no pull-request URL recorded for this entry" => {
            "このエントリにはPRのURLがledgerに記録されていません"
        }
        "check the ledger entry and dispatch a fresh pull request if needed" => {
            "ledgerエントリを確認し、必要であれば新しいPRをdispatchしてください"
        }

        // table::merge::EXACT — "pr_closed_unmerged"
        "the pull request was closed without merging" => "PRはmergeされずにcloseされました",
        "reopen it on the branch, or re-dispatch the task fresh" => {
            "同じブランチで再openするか、タスクを新規にre-dispatchしてください"
        }

        // table::merge::EXACT — "merge_recovery_cap_reached"
        "the automated handback budget for this pull request is spent" => {
            "このPRの自動handback予算を使い切りました"
        }
        "look at the pull request by hand; the automated handback gave up after repeated attempts" => {
            "PRを手動で確認してください。自動handbackは繰り返し試行した末に諦めました"
        }

        // table::merge::PREFIX — merge_dependency::PREREQUISITE_UNMERGED_PREFIX
        "this pull request's task depends on another task that has not merged yet" => {
            "このPRのタスクは、まだmergeされていない別タスクに依存しています"
        }
        "nothing to do until the prerequisite task merges" => {
            "前提タスクがmergeされるまで対応不要です"
        }

        // table::merge::PREFIX — "merge_exit:"
        "`gh pr merge` exited without merging" => "`gh pr merge`はmergeせずに終了しました",
        "press Enter and tell the worker to look at the failing merge and push a fix" => {
            "Enterを押して、失敗しているmergeを確認し修正をpushするようworkerに伝えてください"
        }

        // table::merge::PREFIX — "merge_error:"
        "the merge command itself failed to run" => "mergeコマンド自体の実行に失敗しました",
        "press Enter and tell the worker to look at the branch and push a fix" => {
            "Enterを押して、ブランチを確認し修正をpushするようworkerに伝えてください"
        }

        // table::merge::PREFIX — "merge_refused:"
        "GitHub declined the merge for a reason the worker cannot clear" => {
            "GitHubがworkerでは解消できない理由でmergeを拒否しました"
        }
        "choose a different `merge_strategy`, or merge by hand" => {
            "別の`merge_strategy`を選ぶか、手動でmergeしてください"
        }

        // table::merge::PREFIX — "unprotected:"
        "the base branch or repository fails a protection requirement" => {
            "baseブランチまたはリポジトリがprotectionの要件を満たしていません"
        }
        "fix the branch protection settings, or switch `protection_mode`" => {
            "branch protectionの設定を修正するか、`protection_mode`を切り替えてください"
        }
        _ => return None,
    })
}
