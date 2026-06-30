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

## Derived names

For an agent titled `feature-x` in repo `nex`:

- branch: `nex/feature-x` (`branch_prefix` + title; when unset, `branch_prefix` defaults to
  the sanitized repo name + `/`, e.g. `my.repo` → `my-repo/`)
- worktree: `~/.robco/worktrees/nex_feature-x_<shortid>`
- tmux session: `robco_nex_feature-x` (sanitized; see
  [03-architecture.md](03-architecture.md) tmux naming constraints)
