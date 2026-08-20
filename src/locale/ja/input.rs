//! Japanese translations for `src/ui/input.rs` and `src/ui/input/*.rs` — the
//! keybinding dispatch layer's status messages.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // input/inbox_dismiss.rs
        "dismissed [{}] {}" => "却下しました [{}] {}",
        "inbox is already empty" => "受信箱は既に空です",
        "dismissed {} inbox item(s)" => "受信箱の{}件を却下しました",

        // input.rs
        "created agent {}" => "agentを作成しました: {}",
        "created agent {}, but its repository is no longer registered" => {
            "agentを作成しましたが、そのリポジトリは既に登録から外れています: {}"
        }
        "saved PR prompt to config" => "PRプロンプトをconfigに保存しました",
        "merge in progress: {} — wait or ctrl-c to force quit" => {
            "merge進行中: {} — 待つか、ctrl-cで強制終了してください"
        }
        "launch in progress: {} — wait or ctrl-c to force quit" => {
            "起動処理が進行中: {} — 待つか、ctrl-cで強制終了してください"
        }
        "dismissed merge notice" => "merge通知を閉じました",
        "IME is on; switch to ASCII input" => "IMEが有効です。ASCII入力に切り替えてください",
        _ => return None,
    })
}
