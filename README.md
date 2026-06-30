# RobCo

RobCo is a repo-oriented terminal cockpit for supervising terminal AI agents across
multiple repositories. It discovers git repositories under a launch directory, shows a
`repo -> agents` tree, and runs each agent in its own git worktree and tmux session.

The full product specification lives in [docs/README.md](docs/README.md).

## Run

```bash
cargo run -- ~/abyss
```

Useful checks:

```bash
cargo run -- --list ~/abyss
cargo test
```

## Controls

- `j` / `k` or arrow keys: move selection
- `h` / `l`: collapse or expand a repo
- `n`: create a new agent under the selected repo
- `enter`: attach to the selected agent's tmux session
- `r`: restart the selected agent program
- `x`: kill the selected agent and remove its worktree when the tracked tree is clean
- `q`: quit the cockpit without stopping agents

## dropr Overlay

RobCo is local-first and does not depend on dropr. When the `dropr` CLI is available and
authenticated, RobCo best-effort reads `dropr workspace list`, maps repository remotes to
dropr workspaces, and displays matching workspace metadata in the repo preview. Use
`--no-dropr` to disable this read-only overlay.
