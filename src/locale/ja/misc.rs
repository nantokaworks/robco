//! Japanese translations for scattered single strings: the `OverseerCategory`
//! display label, the `src/overseer/` hint constants that surface in the
//! TUI, and the named empty-state lines.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // model.rs — OverseerCategory::display_label (Inbox only; the other
        // four category names stay English on purpose, see task scope).
        "Waiting on you" => "あなたの対応待ち",

        // src/overseer/mod.rs hint constants, rendered via
        // src/ui/overseer/render.rs and src/ui/input/overseer.rs.
        "dispatch is on but the Overseer daemon is not running — no tasks will be dispatched. Start it with `robco overseer run`, or install the always-on service with `robco overseer install-service`." => {
            "dispatchは有効ですが、Overseer daemonが起動していません — タスクはdispatchされません。`robco overseer run` で起動するか、`robco overseer install-service` で常駐サービスとしてインストールしてください。"
        }
        "dispatch is off — overseer is stopped; press S here to turn dispatch back on" => {
            "dispatchは無効です — overseerは停止中。ここでSを押すとdispatchを再開できます"
        }
        "dispatch circuit is open after repeated worker failures — dispatch stays disabled until you reset it: press [R] here, or run `robco overseer set dispatch on` (re-enables dispatch and clears the failure counter)." => {
            "worker失敗の繰り返しによりdispatch回路がopenになりました — リセットするまでdispatchは無効のままです：ここで[R]を押すか、`robco overseer set dispatch on` を実行してください（dispatchを再有効化し失敗カウンタをクリアします）。"
        }
        "autonomy is full_auto — the merge envelope no longer escalates ambiguous requirements, dependency bumps, large diffs, or prod/CI-config changes; only the hard stops (destructive, security, repeated failures, budget, external side effects) still hold." => {
            "autonomyがfull_autoです — merge envelopeは曖昧な要件、依存関係の更新、大きなdiff、prod/CI設定変更をエスカレーションしなくなります。破壊的操作・セキュリティ・失敗の繰り返し・予算・外部副作用などのhard stopのみ有効です。"
        }

        // Empty-state lines
        "No AI session. Press enter to open one." => {
            "AIセッションがありません。enterで開始できます。"
        }
        "No shell session. Press enter to open one." => {
            "shellセッションがありません。enterで開始できます。"
        }
        "no retained channel agents yet" => "保持されているチャンネルエージェントはまだありません",

        // summary.rs — the one clear prose sentence in the repo summary pane;
        // its surrounding field labels (path/remote/agents/...) stay as
        // structural vocabulary, see the task decision note.
        "no workspace resolved for this repo, so no tasks can be listed" => {
            "このリポジトリに対応するワークスペースが見つからないため、タスクを表示できません"
        }

        // tree/repo_row.rs
        "(no agents)" => "(agentなし)",

        // error_dialog.rs
        "Warning:" => "警告:",
        "f force delete   any other key dismiss" => "fで強制削除   その他のキーで閉じる",
        "press any key to dismiss" => "何かキーを押すと閉じます",

        // preview/branch_only.rs
        "Worktree has been removed." => "worktreeは削除されました。",
        "Press x to delete the branch." => "xを押すとブランチを削除できます。",

        // actions/preview_capture.rs
        "Could not render diff." => "diffを表示できませんでした。",
        _ => return None,
    })
}
