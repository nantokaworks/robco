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
