use ratatui::text::{Line, Span};

use super::theme::DEFAULT as THEME;
use crate::locale::{Locale, t};

pub(crate) fn lines(locale: Locale) -> Vec<Line<'static>> {
    let l = |text: &'static str| Line::from(t(locale, text));
    vec![
        l("Navigation"),
        l("  j/k or arrows  move selection; OVERSEER rows open local control info"),
        l("  h/l            collapse or expand a repo, category, or section"),
        l("  shift-up/down  on a repo row: move it among its sibling repos"),
        l("  tab/shift-tab  cycle the row's preview tabs (info/claude/diff/term)"),
        l("  pgup/pgdn      scroll the preview pane"),
        Line::from(""),
        l("Sessions"),
        l("  n              new agent under selected repo (title | initial prompt)"),
        l("  enter          attach Claude/terminal (agent shell or main worktree)"),
        l("                 on OVERSEER: attach the control AI, creating it if absent"),
        l("                 on a section header: fold or unfold that section"),
        l("  i              on the OVERSEER control AI row: send it an instruction"),
        l("                 on the CLAUDE/CODEX tab: send one line to that session"),
        l("  ctrl-q         return from attached tmux session"),
        l("  r              on a repo row: reload dropr tasks; else restart agent"),
        l("  x              remove an agent worktree, pinned repo, or orphan session"),
        l("  g              on a repo row: rename its local directory"),
        l("  S              kill every overseer worker; the daemon keeps running"),
        l("  R              start the overseer daemon (only when it is not running)"),
        l("  K              stop the overseer daemon process (running)"),
        Line::from(""),
        l("Overseer inbox"),
        l("  l              expand OVERSEER > Inbox to reach its item rows"),
        l("  enter          on an item row: answer the waiting worker"),
        l("  y              on an item row: approve it (sends y + enter)"),
        l("  d              on an item row: dismiss it (hides the row only)"),
        l("  D              on an item or Inbox row: clear the inbox (confirms)"),
        l("                 also: robco inbox clear"),
        Line::from(""),
        l("Overseer discord"),
        l("  l              expand OVERSEER > Discord to reach its channel rows"),
        l("  enter          on a channel row: attach its tmux session (live"),
        l("                 only while a turn is running for that channel)"),
        l("  r              on a channel row: reset a failed channel to idle"),
        l("  x              on a channel row: remove the retained record (confirms)"),
        Line::from(""),
        l("Repo"),
        l("  a              clone <git-url> [branch], or add local repo path"),
        l("  m              land task: open a missing PR, then queue approval"),
        l("                 checks running: queue approval; green: merge now"),
        l("                 failed check: refuse; merged PR: clean up"),
        l("                 PR closed without merging: says to reopen it"),
        l("  p              edit and request PR from selected running agent"),
        l("  c              check out the default branch in the primary checkout"),
        l("                 (clean tree only)"),
        l("  C              clear the repo's own chat session (confirms)"),
        l("                 idle/done only; refuses on a busy session or none live"),
        l("  enter          on a repo row (INFO showing): open its dropr task list"),
        l("                 on a task row: open its body in a popup"),
        l("  n              on a task row: start it now, same as s (skip body)"),
        l("  o              on a task row or its body: open the task in a browser"),
        l("                 over SSH: copies its URL to your clipboard instead"),
        l("  s              on a task body: start the work now (worktree, branch,"),
        l("                 tmux session), claiming it in dropr first"),
        l("  j/k            on a task body: scroll it"),
        l("  esc/h/left     step back up one drill-down level, or close the body"),
        Line::from(""),
        l("Text prompts"),
        l("  left/right     move the cursor within the text being typed"),
        l("  home/ctrl-a    jump to the start; end/ctrl-e jumps to the end"),
        l("  backspace/del  delete before the cursor / at the cursor"),
        l("  ctrl-w/ctrl-u  delete the previous word / back to the line start"),
        Line::from(""),
        l("Indicators"),
        l("  One primary per row: dead > merging > running > waiting > MCP call"),
        l("    > TERM activity > subagents > dropr reload > static status"),
        l("  ⠋… animated agent running   ? waiting   ✗ dead"),
        l("  ⇄ merging   ◐… animated MCP tool call"),
        l("  ⌦ worktree missing (appended after primary; alone if no primary)"),
        l("  merge-failed native merge failed (appended after primary)"),
        l("  blocked worker reported itself blocked (appended after primary)"),
        l("  ▖… animated TERM activity   ✻N active subagents"),
        l("  ⠋… dimmed: manual dropr reload (r key)"),
        l("  ✓ done   · idle   ⎇ branch only (static fallback)"),
        l("  A done row whose PR is open shows the merge state instead:"),
        l("  ◆ approved, waiting on the gate   ↻ checks running"),
        l("  ‼ checks failing   ⏸ held for another reason (INFO says which)"),
        l("  project_icon nerdfont/emoji swaps the fold marker for a folder pair"),
        l("  Collapsed repos: N ⠿ is running; status glyphs/N ⌦ are child counts"),
        l("  Child rows: * uncommitted changes   ⌁ tmux session"),
        Line::from(""),
        l("General"),
        l("  ,              edit settings (config.json) in $EDITOR"),
        l("  ?              show this help"),
        l("  j/k            scroll this help when it does not fit the terminal"),
        l("  q              quit without stopping agents"),
        l("  ctrl-c         quit at once, even while a merge or launch runs"),
        Line::from(""),
        Line::from(Span::styled(
            t(locale, "press any key to close"),
            THEME.hint_style(),
        )),
    ]
}

/// Rows `lines()` emits. Derived, never hand-maintained: this number feeds
/// `max_scroll`, and a constant sitting two hundred lines away from the list
/// it had to match is how the help screen drifted out of date in the first
/// place (dropr:509). Every locale emits one `Line` per entry — the table in
/// `crate::locale::ja::help` translates lines, it never adds or drops one —
/// so the English count is the count.
fn content_line_count() -> u16 {
    lines(Locale::En).len() as u16
}

/// Rows the frame loses around the help content: the 1-row top margin and
/// 1-row footer from `layout::root`, plus the popup's two border rows.
const FRAME_OVERHEAD_ROWS: u16 = 4;

pub(crate) fn max_scroll(frame_height: u16) -> u16 {
    let visible_rows = frame_height.saturating_sub(FRAME_OVERHEAD_ROWS);
    content_line_count().saturating_sub(visible_rows)
}

pub(crate) fn clamp_scroll(scroll: u16, frame_height: u16) -> u16 {
    scroll.min(max_scroll(frame_height))
}

pub(crate) fn scroll_up(scroll: u16, frame_height: u16) -> u16 {
    clamp_scroll(scroll, frame_height).saturating_sub(1)
}

pub(crate) fn scroll_down(scroll: u16, frame_height: u16) -> u16 {
    clamp_scroll(scroll.saturating_add(1), frame_height)
}

pub(crate) fn terminal_height() -> u16 {
    crossterm::terminal::size()
        .map(|(_, height)| height)
        .unwrap_or_else(|_| content_line_count() + FRAME_OVERHEAD_ROWS)
}

pub(crate) fn scroll_title(scroll: u16, frame_height: u16, locale: Locale) -> Option<String> {
    let max = max_scroll(frame_height);
    (max > 0).then(|| {
        crate::locale::fmt(
            locale,
            "help [j/k scroll {}/{}]",
            &[
                &clamp_scroll(scroll, frame_height).to_string(),
                &max.to_string(),
            ],
        )
    })
}

#[cfg(test)]
#[path = "help_tests.rs"]
mod tests;
