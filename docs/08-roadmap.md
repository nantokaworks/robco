# 08 — Roadmap & Scope

## v1 — in scope

The lean, fully-local cockpit:

- **Discovery** — scan the managed repos root plus an optional ephemeral root (depth 1).
- **Project-first tree** — `repo → [agents]` model and UI.
- **Agent lifecycle** — create worktree + branch, spawn `claude` in tmux.
- **Preview** — polled `capture-pane`, color-preserving.
- **Attach / detach** — jump into an agent and back.
- **New (`n`)** — add an arbitrary path / create an agent.
- **Kill (`x`)** — clean-tree check, remove worktree, drop from registry.
- **Registry / reattach** — persist `state.json`; relaunch reattaches.
- **Local status** — running / idle / waiting (heuristic) / dead.

## v1 — explicitly out of scope

Deliberately skipped to keep v1 lean (these are ClaudeSquad extras, not the core need):

- Diff viewer.
- Auto-yes / autopilot daemon.
- Push / PR / GitHub integration.
- Program profiles.
- Cross-machine / remote agents.
- Embedded terminal emulator (tmux owns the PTYs in v1).

## Later — possible evolutions

### Overseer repository scheduling groundwork

The shared managed repos root gives the TUI, CLI, and cwd-less Overseer daemon one stable
repository source. Daemon-side repository scheduling, filesystem watchers, registry IPC,
and single-writer coordination remain deferred; polling and the current registry model
stay unchanged.

### Optional dropr overlay (decoupled, read-only)

RobCo can optionally **overlay state from [dropr](https://github.com/nantokaworks/dropr)**
when a repo maps to a dropr workspace:

- Map repo → workspace via the repo's canonical remote URL.
- Read-only badges from existing dropr endpoints: task linkage, activity/agent-run state,
  and a precise "needs input" signal (replacing the v1 heuristic) once dropr exposes it.
- **Decoupled and best-effort:** RobCo talks to dropr's public API only; if dropr is
  absent, unauthenticated, or offline, the overlay silently disappears and the local
  cockpit works unchanged. This must never pull RobCo into dropr's internals.

This is what makes RobCo *more than* a ClaudeSquad clone for dropr users, while keeping
RobCo independent for everyone else.

### Embedded-PTY mode

A Rust-native alternative to the tmux dependency: own the agent PTYs with
`portable-pty`, parse with `vt100`, render in-TUI with `tui-term`. Cleaner single-binary
UX (preview/attach inside the TUI, no tmux switch) at the cost of owning terminal
emulation, resize handling, and session durability (would need a daemon). Evaluated only
after v1 proves the model.

## Process

RobCo is a **separate repository**, MIT-licensed, independent of dropr. The dropr overlay
phase, if pursued, can be tracked in dropr's own task system at that time.
