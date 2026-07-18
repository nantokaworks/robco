# RobCo — Specification

**RobCo** — *Repo-Oriented Bot Control & Orchestration*

A terminal cockpit that lets you **control and oversee Claude (and other terminal AI
agents) across many repositories at once**, organized **project-first**.

This `docs/` tree is the full specification for RobCo. It is the design source of truth;
the implementation follows it.

## Reading order

| Doc | What it covers |
|-----|----------------|
| [01-overview.md](01-overview.md) | What RobCo is, the gap it fills vs ClaudeSquad, goals / non-goals |
| [02-naming.md](02-naming.md) | Name, tagline, aesthetic, and the legal line |
| [03-architecture.md](03-architecture.md) | Stack, tmux-backed model, module layout, clean-reimplementation note |
| [04-data-model.md](04-data-model.md) | Project-first tree, registry schema, config schema |
| [05-discovery.md](05-discovery.md) | Repo discovery and adding repos/agents |
| [06-ui.md](06-ui.md) | Layout, keybindings, status indicators, aesthetic |
| [07-lifecycle.md](07-lifecycle.md) | worktree × tmux lifecycle, status detection |
| [09-config-reference.md](09-config-reference.md) | Full `~/.robco/config.json` reference — every field, default, and example |
| [10-agent-reporting.md](10-agent-reporting.md) | Agent identity, activity, controller reports, and Overseer inbox routing |
| [11-overseer-agent.md](11-overseer-agent.md) | Autonomous Overseer architecture, configuration, security, setup, and recovery |
| [08-roadmap.md](08-roadmap.md) | v1 scope, explicitly deferred work |

## One-paragraph summary

RobCo is a [ratatui](https://ratatui.rs) TUI that discovers the git repositories under a
directory and presents them as a **`repo → [agents]` tree**. Each agent is a `claude`
process running in its own **git worktree** inside its own **tmux** session — the same
proven mechanics as [ClaudeSquad](https://github.com/smtg-ai/claude-squad), but
**organized around projects** instead of a flat list of instances. You preview any
agent's screen, jump in to interact, and jump back, all from one cockpit. v1 is fully
local with no backend dependency.
