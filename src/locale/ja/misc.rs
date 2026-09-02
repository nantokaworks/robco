//! Japanese translations for scattered single strings: the `src/overseer/`
//! hint constants that surface in the TUI, and the named empty-state lines.
//!
//! UI item labels (headers, field names, status chrome — e.g. the
//! `OverseerCategory` sidebar labels) stay English and have no entry here;
//! only content (sentences, messages, hints) is translated (dropr:377).
//! Any string drawn inside a tree/sidebar row is chrome too and stays
//! English — row placeholders like `(no agents)` or the Discord category's
//! empty-state row belong there, not here (dropr:388).

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
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
        // Empty-state lines
        "No AI session. Press enter to open one." => {
            "AIセッションがありません。enterで開始できます。"
        }
        "No shell session. Press enter to open one." => {
            "shellセッションがありません。enterで開始できます。"
        }
        // PROJECTS remote-host status detail. The host name remains the
        // untranslated label; this sentence is the explanatory content.
        "connecting..." => "接続中…",

        // summary.rs — the one clear prose sentence in the repo summary pane;
        // its surrounding field labels (path/remote/agents/...) stay as
        // structural vocabulary, see the task decision note.
        "no workspace resolved for this repo, so no tasks can be listed" => {
            "このリポジトリに対応するワークスペースが見つからないため、タスクを表示できません"
        }
        "workspace is not materialised — no board exists yet, so no tasks are dispatched or listed for this repo" => {
            "ワークスペースはmaterializeされていません — droprボードがまだ存在しないため、このリポジトリのタスクはdispatchも一覧表示もされません"
        }

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
