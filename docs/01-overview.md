# 01 — Overview

## What RobCo is

RobCo is a terminal cockpit for running and overseeing multiple AI coding agents
(Claude Code by default) **across multiple repositories at the same time**.

It launches each agent in an isolated **git worktree** inside its own **tmux** session,
shows them all in one screen organized **by repository**, lets you **preview** any
agent's live output, and lets you **jump into** any agent to interact and then **jump
back** to the overview.

## The gap it fills

[ClaudeSquad](https://github.com/smtg-ai/claude-squad) (`cs`) already does the per-agent
mechanics well: worktree + agent-in-tmux + preview + attach. But ClaudeSquad is
**instance-first** — its model and UI are a **flat list of worktree/agent instances**.
Managing **multiple repositories/projects together in one session is not its premise**.

RobCo's single defining difference is that it is **repo-oriented (project-first)**: the
organizing unit is the **repository**, and agents hang underneath their repo as a tree:

```
repo → [agent, agent, …]
```

So "oversee N projects, each possibly running several agents, from one cockpit" is the
*premise*, not an afterthought.

## Goals

- Oversee many repositories' agents from a single terminal cockpit.
- Project-first organization: a `repo → agents` tree is the primary structure.
- Reuse ClaudeSquad's proven mechanics (worktree + tmux + capture-pane preview + attach).
- Zero-config start: point it at a directory, it finds the repos.
- Fully local and offline in v1 — no account, no backend.
- Durable sessions: agents survive the cockpit being closed (tmux-backed).

## Non-goals (v1)

- Not a fork of ClaudeSquad — a clean Rust reimplementation referencing its behavior.
- Not a dropr feature and not dependent on dropr. (Optional read-only dropr overlay is a
  later, decoupled add-on — see [08-roadmap.md](08-roadmap.md).)
- Not aiming for full ClaudeSquad feature parity: no diff viewer, auto-yes daemon,
  push/PR integration, or profiles in v1.
- Not a cross-machine / remote cockpit in v1 (everything runs on the local machine).
- Not an embedded terminal emulator in v1 (tmux owns the PTYs).

## Who it is for

Developers — and especially solo builders running several projects at once — who keep
multiple Claude agents going across repos and want one place to watch them, notice which
one needs input, and drop into whichever needs attention.
