# 07 — Lifecycle & Status Detection

## Agent lifecycle (worktree × tmux)

The mechanics mirror ClaudeSquad, reimplemented in Rust.

### Create

1. Resolve the repo and an agent `title`.
2. Compute names: `branch = <branch_prefix><title>` (`branch_prefix` defaults to the sanitized `<repo>/` when unset),
   `worktree = <worktree_root>/<repo>_<title>_<shortid>`,
   `session = <tmux_session_prefix><repo>_<title>` (sanitized).
3. `git -C <repo> worktree add <worktree> -b <branch> <base>` where `<base>` is the repo's
   current `HEAD` (or default branch). Record `base_commit`.
4. `tmux new-session -d -s <session> -c <worktree>` then start the program
   (`default_program`, e.g. `claude`) in that session.
5. Enable `monitor-activity` on the session's window (feeds the Running badge).
6. Append the `AgentNode` to its `RepoNode` and persist the registry.

### Monitor / preview

- Poll loop (default 750 ms): for the selected agent, `tmux capture-pane -e -p` →
  render. For all agents, refresh status (below).

### Attach / detach

- `Enter` → if RobCo runs inside tmux, `tmux switch-client -t <session>`; otherwise
  `tmux attach -t <session>`. Detaching (`prefix + d`) returns to the cockpit.

### Restart

- `r` → restart the program inside the existing session (kill the running pane process,
  relaunch `program`), keeping the worktree.

### Kill

- `x` → confirm, then:
  1. Verify the worktree's tracked tree is clean (uncommitted *tracked* changes → stop and
     warn). Ignored build artifacts may be discarded with an explicit force.
  2. `tmux kill-session -t <session>`.
  3. `git -C <repo> worktree remove <worktree>` then `git worktree prune`.
  4. Remove the `AgentNode` from the registry.

### Quit

- `q` exits RobCo **without** killing agents. tmux sessions keep running; a later `robco`
  reattaches via the registry.

## Reattach on launch

For each persisted agent, reconcile against the live tmux server:

| Live state | Result |
|------------|--------|
| Session exists, program alive | Reattach; status from detection below |
| Session exists, program exited | Show `Dead`; offer restart |
| Session missing | Show `Dead`; offer cleanup (remove worktree) or restart |

## Status detection (local, no backend)

Status is derived entirely from tmux + captured pane text:

- **Running vs Idle** — combine the tmux `monitor-activity` window flag with a diff of
  consecutive `capture-pane` snapshots. Content changed within the recent window ⇒
  `Running`; otherwise `Idle`.
- **Waiting (needs input)** — a **heuristic** over the captured pane text: detect Claude
  Code's permission/confirmation prompt or an idle input prompt at the bottom of the
  pane. This is best-effort and explicitly heuristic.
- **Dead** — tmux session absent or the pane's foreground program has exited.

> The precise, non-heuristic "needs input" signal would require the agent to emit an
> event (e.g. Claude Code hooks → an event stream). That is **out of scope for v1** and
> noted in [08-roadmap.md](08-roadmap.md) as part of the optional dropr overlay.
