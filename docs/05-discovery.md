# 05 — Discovery & Adding Work

## Automatic discovery

On launch, RobCo scans the **immediate subdirectories of the launch directory** and keeps
those that are git repositories.

```
$ cd ~/abyss && robco
# discovers ~/abyss/nex, ~/abyss/dropr, ~/abyss/robco, … (each with a .git)
```

Rules:

- Only the **direct children** of the launch directory are scanned (depth 1). RobCo does
  not recurse into nested trees.
- A child counts as a repo if it contains a `.git` directory or file (worktree/submodule
  form).
- The launch directory can be overridden with a positional argument:
  `robco ~/work` scans `~/work/*`.
- For each discovered repo, RobCo resolves the remote URL (`git -C <dir> remote get-url`,
  preferring `origin`) for display and for the optional future dropr mapping.

A repo is shown in the tree even with **zero agents** — discovery lists projects; agents
are created on demand.

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
