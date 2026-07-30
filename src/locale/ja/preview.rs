//! Japanese translations for `src/ui/preview.rs` and `src/ui/preview/*.rs`.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        // preview.rs
        "no live session — a turn is not running for this channel" => {
            "稼働中のセッションがありません — このチャンネルではturnが実行されていません"
        }
        "No preview available." => "プレビューはありません。",
        "Session is gone." => "セッションは終了しました。",
        "No repositories discovered." => "リポジトリが見つかりませんでした。",
        "Loading diff…" => "diffを読み込み中…",

        // preview/overseer.rs
        "Control session not started." => "制御セッションは開始されていません。",
        "Press i to send an instruction, or Enter to start and attach." => {
            "iで指示を送信、またはEnterで開始してattachできます。"
        }
        _ => return None,
    })
}
