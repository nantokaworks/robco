# 03 — Architecture

## Principle

RobCo is, mechanically, **ClaudeSquad reimplemented in Rust**. The orchestrator is a TUI
that **shells out to `tmux` and `git`**; the agents (`claude`) run inside tmux. The one
structural difference is the **project-first** organization (see
[04-data-model.md](04-data-model.md)).

> **Clean reimplementation, not a fork.** RobCo references ClaudeSquad's *behavior*, not
> its source. ClaudeSquad is AGPL-3.0 (Go); RobCo must **not copy that code**. Mirroring
> observable behavior in independently-written Rust keeps RobCo under its own MIT license
> with no copyleft obligation.

## Stack

| Concern | Choice |
|---------|--------|
| TUI rendering | [`ratatui`](https://ratatui.rs) + `crossterm` |
| Async runtime | `tokio` |
| tmux control | shell out via `std::process::Command` / `tokio::process` |
| git / worktree | shell out to `git` (worktree add/remove/list, remote, status) |
| Registry / config | `serde` + `serde_json` |
| Colored preview | render `tmux capture-pane -e -p` output through [`ansi-to-tui`](https://crates.io/crates/ansi-to-tui) |
| IDs | `nanoid` (per-agent ids) |

### Architecture choice: tmux-backed (v1)

Agents run inside **tmux sessions**, exactly like ClaudeSquad. RobCo does not own the
PTYs or emulate a terminal. Benefits:

- **Durable** — agents survive RobCo crashing/closing; relaunch reattaches.
- **Simple** — no terminal emulation, resize handling, or vt parsing to own.
- **Composable** — preview = `capture-pane`, enter = `attach`/`select-window`.

A Rust-native **embedded-PTY** mode (`portable-pty` + `vt100` + `tui-term`) is a possible
future evolution but is explicitly out of scope for v1 — see
[08-roadmap.md](08-roadmap.md).

## Process model

```
robco (TUI process)
├─ shells out to ──► tmux server
│                      ├─ session robco_<repo>_<agent>  ─► claude  (cwd = worktree)
│                      ├─ session robco_<repo>_<agent>  ─► claude
│                      └─ …
└─ shells out to ──► git   (worktree add/remove, status, remote)
```

- RobCo never embeds the agents; it creates and drives tmux sessions.
- Preview is **pull-based**: RobCo polls `capture-pane` for the selected agent on an
  interval (default 750 ms).
- Entering an agent attaches/switches the terminal to that tmux session; detaching
  returns to the RobCo cockpit.

## Proposed module layout

```
robco/
├ Cargo.toml
├ LICENSE
├ docs/
└ src/
   ├ main.rs        # entry: parse args, boot screen, run app
   ├ cli.rs         # clap args (launch dir, flags)
   ├ config.rs      # ~/.robco/config.json load/defaults
   ├ model.rs       # RepoNode, AgentNode, Status enums
   ├ registry.rs    # ~/.robco/state.json load/save, tree ops, reattach
   ├ discover.rs    # scan a dir for git repos
   ├ git.rs         # worktree add/remove/list, remote url, clean-tree check
   ├ tmux.rs        # tmux command wrappers (session/window/capture/attach/kill)
   ├ status.rs      # local status detection (running/idle/waiting/dead)
   ├ agent.rs       # spawn/kill/restart an agent (git + tmux orchestration)
   └ ui/
      ├ mod.rs      # app state + event loop
      ├ tree.rs     # left pane: repo → agents tree
      ├ preview.rs  # right pane: capture-pane render
      ├ input.rs    # key handling, modal prompts (e.g. `n` new-path)
      └ theme.rs    # retro green-terminal styling
```

## Testability

Keep all side-effecting `tmux`/`git` calls behind thin wrappers in `tmux.rs` / `git.rs`
that build the argument vectors, so the **command construction is unit-testable** without
spawning real processes. `discover.rs` filtering is unit-testable against temp dirs.

## tmux naming constraints

tmux target syntax treats `:` and `.` specially (`session:window.pane`). Session names
must therefore avoid `:` and `.`. RobCo composes session names from sanitized parts:

```
robco_<sanitized-repo>_<sanitized-agent>
```

where sanitization replaces any disallowed characters with `-`.
