//! Japanese translations for the readable sentences
//! `src/overseer/discord/humanize.rs`'s `EXACT` / `PREFIX` tables map a
//! code-shaped decision reason onto. The Inbox preview's reason section
//! shows this sentence, localized, ahead of the raw reason code (dropr:400)
//! — the raw code itself is never translated, so it stays a stable anchor
//! back to the source.
//!
//! Only the vocabulary-table sentences are covered here.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // humanize::EXACT
        "A worker started working on this task." => "workerがこのタスクの作業を開始しました。",
        "The worker opened a pull request." => "workerがPRをopenしました。",
        "The pull request was merged." => "PRがmergeされました。",
        "The worker failed to finish this task." => "workerがこのタスクを完了できませんでした。",
        "This task needs an operator's decision." => "このタスクにはoperatorの判断が必要です。",
        "The worker is blocked and asked for help." => {
            "workerがblockedになり、助けを求めています。"
        }
        "The task queue is empty." => "タスクキューは空です。",
        "The pull request's checks are still running." => "PRのchecksはまだ実行中です。",
        "A required check failed on the pull request." => "PRで必須checkが失敗しました。",
        "The pull request is waiting for its turn in the merge queue." => {
            "PRはmerge queueで順番待ちです。"
        }
        "The branch was updated from its base, so the checks run again." => {
            "ブランチがbaseから更新されたため、checksが再実行されます。"
        }
        "The budget for updating this branch from its base is used up." => {
            "このブランチをbaseから更新するための予算を使い切りました。"
        }
        "The branch has conflicts with its base." => "ブランチがbaseと競合しています。",
        "The branch is missing a required review or check." => {
            "ブランチに必須のreviewまたはcheckが不足しています。"
        }
        "No pull request URL is recorded for this task." => {
            "このタスクにはPRのURLが記録されていません。"
        }
        "The pull request was closed without merging." => "PRはmergeされずにcloseされました。",
        "The automated handback gave up after repeated attempts." => {
            "自動handbackは繰り返し試行した末に諦めました。"
        }
        "Repeated spawn failures tripped the circuit for this task." => {
            "起動失敗が繰り返され、このタスクの回路がtripしました。"
        }

        // humanize::PREFIX
        "The pull request was held on the same condition too many times." => {
            "PRが同じ条件で何度も保留されました。"
        }
        "The pull request has been stuck on the same hold for hours." => {
            "PRが同じholdのまま何時間も動いていません。"
        }
        "The recheck budget for this hold is used up." => {
            "このholdの再チェック予算を使い切りました。"
        }
        "This task waits for another task's pull request to merge first." => {
            "このタスクは、別タスクのPRが先にmergeされるのを待っています。"
        }
        "Another agent already holds this task." => "別のagentが既にこのタスクを保持しています。",
        "The daemon could not start a worker for this task." => {
            "daemonがこのタスクのworker起動に失敗しました。"
        }
        "The failure circuit is open, so dispatch is stopped." => {
            "失敗回路がopenのため、dispatchは停止しています。"
        }
        "Repeated failures are close to opening the circuit." => {
            "連続した失敗が回路を開く一歩手前です。"
        }
        "The merge command exited without merging." => "mergeコマンドはmergeせずに終了しました。",
        "The merge command failed to run." => "mergeコマンドの実行に失敗しました。",
        "GitHub refused the merge." => "GitHubがmergeを拒否しました。",
        "The base branch does not meet a protection requirement." => {
            "baseブランチがprotectionの要件を満たしていません。"
        }
        "Updating the branch from its base failed." => "ブランチをbaseから更新できませんでした。",
        "The automated handback was skipped this pass." => {
            "今回のpassでは自動handbackがスキップされました。"
        }
        "This looks worker-fixable, but automated handback is turned off." => {
            "worker側で修正できそうですが、自動handbackが無効になっています。"
        }
        "The handback message did not reach the worker." => {
            "handbackのメッセージがworkerに届きませんでした。"
        }
        "The session could not sign in." => "セッションがsign inできませんでした。",
        _ => return None,
    })
}
