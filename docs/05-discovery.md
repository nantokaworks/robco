# 05 — Discovery & Adding Work

## Managed repositories and discovery

RobCo's persistent repository source is `repos_root` (default `~/.robco/repos`). Bare
`robco` scans only its immediate children. The directory may be absent or empty; first
launch still succeeds.

```
$ robco add https://host/owner/nex.git
# clones ~/.robco/repos/nex and immediately pins it in state.json
```

Rules:

- Only the **direct children** of each root are scanned (depth 1); discovery never
  recurses.
- A child counts as a repo if it contains a `.git` directory or file (worktree/submodule
  form).
- Bare `robco` scans `{repos_root}` and no longer scans the current directory. Use
  `robco .` to include the current directory explicitly.
- `robco <dir>` scans `{repos_root} ∪ {dir}`. The positional directory is ephemeral for
  that session and is never written to config or `state.json`.
- Missing or unreadable roots are skipped, and repositories found through multiple
  roots are deduplicated by canonical path.
- For each discovered repo, RobCo resolves the remote URL (`git -C <dir> remote get-url`,
  preferring `origin`) for display and for the optional future dropr mapping.

A repo is shown in the tree even with **zero agents** — discovery lists projects; agents
are created on demand.

## Cloning and adding repositories

From the command line, `robco add <url> [--branch <branch>] [--name <name>]` clones into
`repos_root` and immediately registers the result as pinned. URLs are host-agnostic:
HTTP(S), SSH, git, file, and scp-style forms are handed directly to git for authentication.

Inside the cockpit, **`a`** prompts for `<git-url> [branch]`. URL input clones in the
background into `repos_root`; a local git-repository path keeps the legacy pinned-add
behavior.

## Adding a repo by path (`n`)

Inside the cockpit, pressing **`n`** opens a prompt to enter an arbitrary path. This:

- adds a repository that lives outside the launch directory, or
- (when a repo node is selected) creates a **new agent** under that repo.

The exact binding semantics:

| Selection when `n` is pressed | Action |
|-------------------------------|--------|
| A repo node | Create a new agent under that repo (prompt for an agent title) |
| An agent node | Create a sibling agent under the same repo |
| Empty / root | Prompt for a path → add a repo (and optionally a first agent) |

## What "create an agent" does

Creating an agent triggers the worktree × tmux lifecycle in
[07-lifecycle.md](07-lifecycle.md): a new worktree + branch is created and `claude` is
started inside a fresh tmux session, then the agent appears under its repo in the tree.

## Claude Code subagent discovery

RobCo passively reads Claude Code's local session files; it does not start, stop, or
message subagents. For a worktree path, it converts every non-ASCII-alphanumeric character
to `-` and looks under:

```text
~/.claude/projects/<worktree-slug>/
```

Among the direct `*.jsonl` session files modified in the last 10 minutes, RobCo selects
the newest. It then reads matching
`<session-id>/subagents/agent-<id>.meta.json` and `agent-<id>.jsonl` files. Metadata supplies
the subagent type, description, and spawn depth; the activity JSONL file's modification
time supplies liveness:

- modified within about 60 seconds: **Running** and included in `✻N`;
- older than 60 seconds but no more than 10 minutes: **Done**, retained in the agent INFO
  pane but not counted;
- older than 10 minutes: omitted.

These files are an undocumented Claude Code format and may change. Missing directories,
unreadable files, malformed metadata, unmatched activity files, and implausible future
timestamps are ignored defensively. Detection then degrades silently to no indicator;
agent operation is unaffected.

RobCo also applies lifecycle guards before reading. Dead and branch-only agents, agents
whose worktree directory is missing, and repo main sessions that are not live produce no
subagent activity. This prevents stale session files from keeping an indicator visible.
