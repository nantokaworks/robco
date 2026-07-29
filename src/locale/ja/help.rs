//! Japanese translations for `src/ui/help.rs` — the `?` help screen.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        "Navigation" => "操作",
        "  j/k or arrows  move selection; OVERSEER rows open local control info" => {
            "  j/k または矢印  選択を移動。OVERSEER行はローカル制御情報を開く"
        }
        "  h/l            collapse or expand repo or OVERSEER category" => {
            "  h/l            リポジトリ/OVERSEERカテゴリの折りたたみ・展開"
        }
        "  shift-up/down  on a repo row: move it among its sibling repos" => {
            "  shift-up/down  リポジトリ行：兄弟リポジトリ間で並び替え"
        }
        "  tab/shift-tab  cycle claude / diff / terminal view" => {
            "  tab/shift-tab  claude / diff / terminal 表示を切り替え"
        }
        "Sessions" => "セッション",
        "  n              new agent under selected repo (title | initial prompt)" => {
            "  n              新規エージェント作成（タイトル | 初期プロンプト）"
        }
        "  enter          attach Claude/terminal (agent shell or main worktree)" => {
            "  enter          Claude/terminalへアタッチ（エージェントshell/main）"
        }
        "  ctrl-q         return from attached tmux session" => {
            "  ctrl-q         アタッチ中のtmuxセッションから戻る"
        }
        "  r              on a repo row: reload dropr tasks; else restart agent" => {
            "  r              リポジトリ行：droprタスク再読込／他はエージェント再起動"
        }
        "  x              remove selected agent worktree or pinned repo" => {
            "  x              選択したworktreeまたは固定リポジトリを削除"
        }
        "  g              cycle selected worktree: unmanaged -> Auto -> Manual" => {
            "  g              worktreeの状態を切替：unmanaged -> Auto -> Manual"
        }
        "                 on a repo row: any Auto -> all Manual, else all Auto" => {
            "                 リポジトリ行：一部Auto -> 全Manual、他は全Auto"
        }
        "  G              on a repo row: toggle Overseer opt-out for the whole repo" => {
            "  G              リポジトリ行：リポジトリ全体のOverseer対象外を切替"
        }
        "                 (opted out: no dispatch/merge; running workers untouched)" => {
            "                 （対象外：dispatch/mergeなし。稼働中workerは継続）"
        }
        "  S              stop overseer (kill workers); off: start dispatch" => {
            "  S              overseer停止（worker強制終了）／off時：dispatch開始"
        }
        "  R              reset dispatch circuit (when open)" => {
            "  R              dispatch回路をリセット（open時）"
        }
        "Overseer inbox" => "Overseer受信箱",
        "  l              expand OVERSEER > Inbox to reach its item rows" => {
            "  l              OVERSEER > Inboxを展開し項目行を表示"
        }
        "  enter          on an item row: answer the waiting worker" => {
            "  enter          項目行：待機中のworkerに回答"
        }
        "  y              on an item row: approve it (sends y + enter)" => {
            "  y              項目行：承認（y + enterを送信）"
        }
        "  d              on an item row: dismiss it (hides the row only)" => {
            "  d              項目行：却下（この行のみ非表示）"
        }
        "  D              on an item or Inbox row: clear the inbox (confirms)" => {
            "  D              項目またはInbox行：Inboxを全消去（要確認）"
        }
        "                 also: robco overseer clear-inbox" => {
            "                 同等コマンド: robco overseer clear-inbox"
        }
        "Repo" => "リポジトリ",
        "  a              clone <git-url> [branch], or add local repo path" => {
            "  a              <git-url> [branch] をclone、またはローカルパスを追加"
        }
        "  m              merge agent: merge PR + pull main (commit + PR needed)" => {
            "  m              エージェントをmerge：PR mergeとmain pull（commit+PR必須）"
        }
        "                 already-merged PR: clean up without merging again" => {
            "                 merge済みPR：再mergeせずクリーンアップのみ"
        }
        "  p              edit and request PR from selected running agent" => {
            "  p              選択中の稼働エージェントからPRを編集・依頼"
        }
        "Text prompts" => "テキスト入力",
        "  left/right     move the cursor within the text being typed" => {
            "  left/right     入力中テキスト内でカーソルを移動"
        }
        "  home/ctrl-a    jump to the start; end/ctrl-e jumps to the end" => {
            "  home/ctrl-a    先頭へ移動／end/ctrl-eで末尾へ移動"
        }
        "  backspace/del  delete before the cursor / at the cursor" => {
            "  backspace/del  カーソル前 / カーソル位置を削除"
        }
        "  ctrl-w/ctrl-u  delete the previous word / back to the line start" => {
            "  ctrl-w/ctrl-u  直前の単語を削除 / 行頭まで削除"
        }
        "Indicators" => "インジケーター",
        "  One primary per row: dead > running > waiting > TERM activity" => {
            "  行ごとの主表示は1つ：dead > running > waiting > TERM activity"
        }
        "    > subagents > dropr reload > static status" => {
            "    > subagents > dropr reload > static status の順"
        }
        "  ⠋… animated agent running   ? waiting   ✗ dead" => {
            "  ⠋… アニメーションはエージェント実行中   ? 待機中   ✗ 停止"
        }
        "  ⌦ worktree missing (appended after primary; alone if no primary)" => {
            "  ⌦ worktreeなし（主表示の後に付加。主表示がなければ単独表示）"
        }
        "  merge-failed native merge failed (appended after primary)" => {
            "  merge-failed ネイティブmerge失敗（主表示の後に付加）"
        }
        "  ▖… animated TERM activity   ✻N active subagents" => {
            "  ▖… アニメーションはTERM活動中   ✻N は稼働中サブエージェント数"
        }
        "  ⟳ manual dropr reload (r key)" => "  ⟳ 手動dropr再読込（rキー）",
        "  ✓ done   · idle   ⎇ branch only (static fallback)" => {
            "  ✓ 完了   · アイドル   ⎇ branchのみ（静的フォールバック）"
        }
        "  ● overseer Auto   ○ overseer Manual   blank unmanaged (rides indent)" => {
            "  ● overseer Auto   ○ overseer Manual   空欄はunmanaged（インデントに従う）"
        }
        "  nerdfont project_icon swaps in a bolt/hand pictograph pair instead" => {
            "  nerdfont有効時はproject_iconが稲妻/手のペア絵文字に置換"
        }
        "  Repo row always shows its own marker; Auto agent rows always show" => {
            "  リポジトリ行は常に自身のマーカーを表示。Autoエージェント行も常に"
        }
        "  theirs too; a Manual agent row blanks only when its repo is Manual" => {
            "  表示。Manualエージェント行は所属リポジトリがManualの時のみ空欄"
        }
        "  Opted-out repo (○): name and its agent rows render dimmed" => {
            "  対象外リポジトリ（○）：名前とエージェント行は薄く表示"
        }
        "  Collapsed repos: N ⠿ is running; status glyphs/N ⌦ are child counts" => {
            "  折りたたみ済みリポジトリ：N ⠿ は実行数、他の記号/N ⌦ は子の数"
        }
        "  Child rows: * uncommitted changes   ⌁ tmux session" => {
            "  子行：* 未コミット変更あり   ⌁ tmuxセッションあり"
        }
        "General" => "全般",
        "  ,              edit settings (config.json) in $EDITOR" => {
            "  ,              $EDITORで設定（config.json）を編集"
        }
        "  ?              show this help" => "  ?              このヘルプを表示",
        "  q              quit without stopping agents" => {
            "  q              エージェントを停止せずに終了"
        }
        "press any key to close" => "何かキーを押すと閉じます",
        "help [j/k scroll {}/{}]" => "ヘルプ [j/k でスクロール {}/{}]",
        _ => return None,
    })
}
