# 04 — Data Model

The defining structure is the **project-first tree**: repositories at the top level, with
agents nested under their repo.

```
RobCo registry (tree)
└ repo: ~/abyss/nex
    ├ agent "feature-x"  → worktree ~/.robco/worktrees/nex_feature-x_ab12cd
    │                      branch nex/feature-x   tmux robco_nex_feature-x
    └ agent "bugfix-y"   → worktree …               branch …   tmux …
└ repo: ~/abyss/dropr
    └ agent "main"       → …
```

## In-memory model (`model.rs`)

```rust
struct RepoNode {
    path: PathBuf,          // absolute repo path
    name: String,           // display name (repo dir name)
    remote_url: Option<String>,
    agents: Vec<AgentNode>,
}

struct AgentNode {
    id: String,             // nanoid
    title: String,          // human label, also feeds branch/session names
    worktree_path: PathBuf,
    branch: String,
    base_commit: String,    // commit the worktree branched from
    program: String,        // e.g. "claude"
    tmux_session: String,   // robco_<repo>_<agent>
    status: Status,         // derived at runtime, not persisted authoritatively
}

enum Status {
    Idle,      // alive, no recent output
    Running,   // producing output
    Waiting,   // appears to be waiting for user input (heuristic)
    Dead,      // tmux session gone / program exited
}
```

## Registry — `~/.robco/state.json`

Persisted so a relaunch reattaches to existing tmux sessions instead of duplicating them.

```json
{
  "version": 1,
  "repos": [
    {
      "path": "/Users/ich/abyss/nex",
      "name": "nex",
      "remote_url": "git@github.com:nantokaworks/nex.git",
      "agents": [
        {
          "id": "V1StGXR8_Z5j",
          "title": "feature-x",
          "worktree_path": "/Users/ich/.robco/worktrees/nex_feature-x_ab12cd",
          "branch": "nex/feature-x",
          "base_commit": "9d04162009e4a375df83c870ad1550fed331686a",
          "program": "claude",
          "tmux_session": "robco_nex_feature-x",
          "created_at": "2026-06-30T09:30:00+09:00",
          "updated_at": "2026-06-30T09:31:00+09:00"
        }
      ]
    }
  ]
}
```

Notes:

- `status` is **not** persisted as truth — on launch RobCo reconciles each persisted
  agent against the live tmux server (session present? program alive?) and recomputes.
- A persisted agent whose tmux session no longer exists is shown as `Dead` and can be
  cleaned up or restarted.

## UI state — `~/.robco/ui-state.json`

The sidebar layout the operator arranged: expand/collapse flags and the PROJECTS order.
See [06-ui.md](06-ui.md#sidebar-layout) for what each one does on screen.

```json
{
  "collapsed_repos": ["/Users/ich/abyss/nex"],
  "expanded_children": ["/Users/ich/.robco/worktrees/nex_feature-x_ab12cd"],
  "other_collapsed": false,
  "orphans_collapsed": false,
  "expanded_overseer_categories": ["Discord"],
  "project_order": ["/Users/ich/abyss/nex", "/Users/ich/abyss/other"]
}
```

Notes:

- Deliberately **not** part of `state.json`, which discovery rewrites on every refresh, and
  not part of `config.json`, which holds settings the operator edits by hand.
- Every entry is keyed by canonical path — a repo path or an agent worktree path — never by
  index, so a registry the next scan reorders cannot land a flag on the wrong row. A key
  that no longer matches anything is ignored and pruned on the next write.
- `collapsed_repos` records the exception rather than the rule, so a repo the file has never
  seen starts expanded, matching how a fresh scan treats it.
- Cosmetic state only: a missing, unreadable, or corrupt file degrades to defaults instead
  of blocking startup, and a failed write costs a layout tweak and nothing else. Writes are
  atomic (temp file + rename) and last-writer-wins; a single TUI owns the file.

## Config — `~/.robco/config.json`

```json
{
  "default_program": "claude",
  "worktree_root": "~/.robco/worktrees",
  "tmux_session_prefix": "robco_",
  "poll_interval_ms": 750
}
```

| Key | Meaning | Default |
|-----|---------|---------|
| `default_program` | Program launched per agent | `claude` |
| `branch_prefix` | Prefix for created branches; omit to derive `<repo>/` from the (sanitized) project name | (unset → `<repo>/`) |
| `worktree_root` | Where worktrees are created | `~/.robco/worktrees` |
| `tmux_session_prefix` | Prefix for tmux session names | `robco_` |
| `poll_interval_ms` | Preview/status poll cadence | `750` |

> This table lists only the core keys. See [09-config-reference.md](09-config-reference.md)
> for the **complete, annotated reference** of every field (`profiles`, `dropr_overlay`,
> `auto_accept`, `merge_strategy`, `notify`, `project_icon`, …) with defaults and examples.

## Derived names

For an agent titled `feature-x` in repo `nex`:

- branch: `nex/feature-x` (`branch_prefix` + title; when unset, `branch_prefix` defaults to
  the sanitized repo name + `/`, e.g. `my.repo` → `my-repo/`)
- worktree: `~/.robco/worktrees/nex_feature-x_<shortid>`
- tmux session: `robco_nex_feature-x` (sanitized; see
  [03-architecture.md](03-architecture.md) tmux naming constraints)
