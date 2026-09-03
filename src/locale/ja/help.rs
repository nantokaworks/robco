//! Japanese translations for `src/ui/help.rs` — the `?` help screen.
//!
//! UI item labels (headers, field names, status chrome — e.g. the section
//! headers like `Navigation` / `Sessions` / `General` and the title chrome)
//! stay English and have no entry here; only content (the key-description
//! lines and hints) is translated (dropr:377).
//!
//! Entries are kept in the order `help::lines` emits them, so a line added
//! or reworded there is easy to find here. A line with no entry falls back to
//! English, which is why the pure symbol-and-term rows have none.

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    Some(match en {
        "  j/k or arrows  move selection; OVERSEER rows open local control info" => {
            "  j/k または矢印  選択を移動。OVERSEER行はローカル制御情報を開く"
        }
        "  h/l            collapse or expand a repo, category, or section" => {
            "  h/l            リポジトリ／カテゴリ／セクションの折りたたみ・展開"
        }
        "  shift-up/down  on a repo row: move it among its sibling repos" => {
            "  shift-up/down  リポジトリ行：兄弟リポジトリ間で並び替え"
        }
        "  tab/shift-tab  cycle the row's preview tabs (info/claude/diff/term)" => {
            "  tab/shift-tab  その行のプレビュータブを切り替え"
        }
        "  pgup/pgdn      scroll the preview pane" => "  pgup/pgdn      プレビューをスクロール",
        "  n              new agent under selected repo (title | initial prompt)" => {
            "  n              新規エージェント作成（タイトル | 初期プロンプト）"
        }
        "  enter          attach Claude/terminal (agent shell or main worktree)" => {
            "  enter          Claude/terminalへアタッチ（エージェントshell/main）"
        }
        "                 on OVERSEER: attach the control AI, creating it if absent" => {
            "                 OVERSEER：制御AIにアタッチ（未作成なら新規作成）"
        }
        "                 on a section header: fold or unfold that section" => {
            "                 セクション見出し：そのセクションを開閉"
        }
        "  i              on the OVERSEER control AI row: send it an instruction" => {
            "  i              OVERSEER制御AI行：指示を送信"
        }
        "                 on the CLAUDE/CODEX tab: send one line to that session" => {
            "                 CLAUDE/CODEXタブ：そのセッションへ1行送信"
        }
        "  ctrl-q         return from attached tmux session" => {
            "  ctrl-q         アタッチ中のtmuxセッションから戻る"
        }
        "  r              on a repo row: reload dropr tasks; else restart agent" => {
            "  r              リポジトリ行：droprタスク再読込／他はエージェント再起動"
        }
        "  x              remove an agent worktree, pinned repo, or orphan session" => {
            "  x              worktree・固定リポジトリ・孤立セッションを削除"
        }
        "  g              on a repo row: rename its local directory" => {
            "  g              リポジトリ行：ローカルディレクトリの名前を変更"
        }
        "  S              kill every overseer worker; the daemon keeps running" => {
            "  S              overseer workerを全て終了（daemonは継続）"
        }
        "  R              start the overseer daemon (only when it is not running)" => {
            "  R              overseerデーモンを起動（停止中のみ）"
        }
        "  K              stop the overseer daemon process (running)" => {
            "  K              overseerデーモンプロセスを停止（稼働中）"
        }
        "  H              connect to a host by ssh destination" => {
            "  H              SSH接続先を指定してホストへ接続"
        }
        "                 use --host at startup or config.hosts to persist it" => {
            "                 起動時は--host、永続化にはconfig.hostsを使用"
        }
        "                 control AI and Discord chats appear after its repos" => {
            "                 制御AIとDiscordチャットはリポジトリの後に表示"
        }
        "  y              on a worker row: approve the newest actionable escalation" => {
            "  y              worker行：最新の対応可能なエスカレーションを承認"
        }
        "                 (sends y + enter)" => "                 （y + enterを送信）",
        "  d              on a worker row: dismiss its newest escalation" => {
            "  d              worker行：最新のエスカレーションを却下"
        }
        "                 (hides the escalation only)" => {
            "                 （エスカレーションのみ非表示）"
        }
        "                 also: robco inbox clear" => {
            "                 同等コマンド: robco inbox clear"
        }
        "  l              expand OVERSEER > Discord to reach its channel rows" => {
            "  l              OVERSEER > Discordを展開しチャンネル行を表示"
        }
        "  enter          on a channel row: attach its tmux session (live" => {
            "  enter          チャンネル行：tmuxセッションへアタッチ（ライブなのは"
        }
        "                 only while a turn is running for that channel)" => {
            "                 そのチャンネルでターン実行中のみ）"
        }
        "  r              on a channel row: reset a failed channel to idle" => {
            "  r              チャンネル行：failed状態をidleにリセット"
        }
        "  x              on a channel row: remove the retained record (confirms)" => {
            "  x              チャンネル行：保持レコードを削除（要確認）"
        }
        "  a              clone <git-url> [branch], or add local repo path" => {
            "  a              <git-url> [branch] をclone、またはローカルパスを追加"
        }
        "  m              land task: open a missing PR, then queue approval" => {
            "  m              タスクをland：PRがなければ作成後、承認をキューへ追加"
        }
        "                 checks running: queue approval; green: merge now" => {
            "                 チェック実行中：承認をキューへ追加；green：即merge"
        }
        "                 failed check: refuse; merged PR: clean up" => {
            "                 チェック失敗：拒否；merge済みPR：クリーンアップ"
        }
        "                 PR closed without merging: says to reopen it" => {
            "                 未mergeでcloseされたPR：再openを促す"
        }
        "  u              update a behind PR's branch from its base (GitHub-side)" => {
            "  u              behind状態のPRブランチをbaseから更新（GitHub側で実行）"
        }
        "  p              edit and request PR from selected running agent" => {
            "  p              選択中の稼働エージェントからPRを編集・依頼"
        }
        "  c              check out the default branch in the primary checkout" => {
            "  c              primary checkoutを既定ブランチへ切替"
        }
        "                 (clean tree only)" => "                 （作業ツリーがclean時のみ）",
        "  C              clear the repo's own chat session (confirms)" => {
            "  C              リポジトリ自身のチャットセッションをクリア（確認あり）"
        }
        "                 idle/done only; refuses on a busy session or none live" => {
            "                 idle/done時のみ；稼働中またはセッション未起動時は拒否"
        }
        "  enter          on a repo row (INFO showing): open its dropr task list" => {
            "  enter          リポジトリ行（INFO表示中）：droprタスク一覧を開く"
        }
        "                 on a task row: open its body in a popup" => {
            "                 タスク行：本文をポップアップで開く"
        }
        "  n              on a task row: start it now, same as s (skip body)" => {
            "  n              タスク行：sと同じく即座に開始（本文なし）"
        }
        "  o              on a task row or its body: open the task in a browser" => {
            "  o              タスク行/本文：ブラウザでタスクを開く"
        }
        "                 over SSH: copies its URL to your clipboard instead" => {
            "                 SSH経由：代わりにURLをクリップボードへコピー"
        }
        "  s              on a task body: start the work now (worktree, branch," => {
            "  s              タスク本文：即座に作業開始（worktree・branch・"
        }
        "                 tmux session), claiming it in dropr first" => {
            "                 tmuxセッション）。先にdroprでclaim"
        }
        "  j/k            on a task body: scroll it" => "  j/k            タスク本文：スクロール",
        "  esc/h/left     step back up one drill-down level, or close the body" => {
            "  esc/h/left     ドリルダウンを1段階戻る／本文を閉じる"
        }
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
        "  One primary per row: dead > merging > running > waiting > MCP call" => {
            "  行ごとの主表示は1つ：dead > merging > running > waiting > MCP call"
        }
        "    > TERM activity > subagents > dropr reload > static status" => {
            "    > TERM activity > subagents > dropr reload > static status の順"
        }
        "  ⠋… animated agent running   ? waiting   ✗ dead" => {
            "  ⠋… アニメーションはエージェント実行中   ? 待機中   ✗ 停止"
        }
        "  ⇄ merging   ◐… animated MCP tool call" => {
            "  ⇄ merge実行中   ◐… アニメーションはMCP呼び出し"
        }
        "  ⌦ worktree missing (appended after primary; alone if no primary)" => {
            "  ⌦ worktreeなし（主表示の後に付加。主表示がなければ単独表示）"
        }
        "  merge-failed native merge failed (appended after primary)" => {
            "  merge-failed ネイティブmerge失敗（主表示の後に付加）"
        }
        "  blocked worker reported itself blocked (appended after primary)" => {
            "  blocked workerが自己申告でblocked（主表示の後に付加）"
        }
        "  ▖… animated TERM activity   ✻N active subagents" => {
            "  ▖… アニメーションはTERM活動中   ✻N は稼働中サブエージェント数"
        }
        "  ⠋… dimmed: manual dropr reload (r key)" => "  ⠋… 薄色表示は手動dropr再読込（rキー）",
        "  ✓ done   · idle   ⎇ branch only (static fallback)" => {
            "  ✓ 完了   · アイドル   ⎇ branchのみ（静的フォールバック）"
        }
        "  A done row whose PR is open shows the merge state instead:" => {
            "  PRがopenのdone行は代わりにmerge状態を表示："
        }
        "  ◆ approved, waiting on the gate   ↻ checks running" => {
            "  ◆ 承認済みでゲート待ち   ↻ チェック実行中"
        }
        "  ‼ checks failing   ⏸ held for another reason (INFO says which)" => {
            "  ‼ チェック失敗   ⏸ その他の理由で保留（INFOに詳細）"
        }
        "  project_icon nerdfont/emoji swaps the fold marker for a folder pair" => {
            "  project_icon が nerdfont/emoji の時は開閉マーカーをフォルダに置換"
        }
        "  Collapsed repos: N ⠿ is running; status glyphs/N ⌦ are child counts" => {
            "  折りたたみ済みリポジトリ：N ⠿ は実行数、他の記号/N ⌦ は子の数"
        }
        "  Child rows: * uncommitted changes   ⌁ tmux session" => {
            "  子行：* 未コミット変更あり   ⌁ tmuxセッションあり"
        }
        "  ,              edit settings (config.json) in $EDITOR" => {
            "  ,              $EDITORで設定（config.json）を編集"
        }
        "  ?              show this help" => "  ?              このヘルプを表示",
        "  j/k            scroll this help when it does not fit the terminal" => {
            "  j/k            画面に収まらない時はこのヘルプをスクロール"
        }
        "  q              quit without stopping agents" => {
            "  q              エージェントを停止せずに終了"
        }
        "  ctrl-c         quit at once, even while a merge or launch runs" => {
            "  ctrl-c         merge/起動の実行中でも即座に終了"
        }
        "press any key to close" => "何かキーを押すと閉じます",
        _ => return None,
    })
}
