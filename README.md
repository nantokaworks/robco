# RobCo

RobCo is a repo-oriented terminal cockpit for supervising terminal AI agents across
multiple repositories. It discovers git repositories under a launch directory, shows a
`repo -> agents` tree, and runs each agent in its own git worktree and tmux session.

The full product specification lives in [docs/README.md](docs/README.md).

## Run

```bash
cargo run -- ~/abyss
cargo run -- --autoyes ~/abyss
```

## Installation

### Homebrew

```bash
brew install nantokaworks/tap/robco
```

Upgrade later with:

```bash
brew upgrade nantokaworks/tap/robco
```

### Pre-built Binaries

Release archives are published from the local maintainer pipeline to
[`nantokaworks/robco-releases`](https://github.com/nantokaworks/robco-releases):

- `robco-<version>-aarch64-apple-darwin.tar.gz`
- `robco-<version>-x86_64-apple-darwin.tar.gz`
- `robco-<version>-aarch64-unknown-linux-gnu.tar.gz`
- `robco-<version>-x86_64-unknown-linux-gnu.tar.gz`

### Build From Source

```bash
git clone https://github.com/nantokaworks/robco
cd robco
cargo install --path .
```

Useful checks:

```bash
cargo run -- --list ~/abyss
cargo run -- debug
cargo test
```

## Controls

- `j` / `k` or arrow keys: move selection
- `h` / `l`: collapse or expand a repo
- `page up` / `page down`: scroll the preview, diff, or help pane
- `n`: create a new agent under the selected repo
- `N`: create a new agent with an initial prompt (`title | prompt`)
- `a`: add an arbitrary repository path
- `enter`: attach to the selected agent's tmux session
- `ctrl-q`: return from the attached tmux session to RobCo
- `tab`: switch between terminal preview and git diff
- `r`: restart the selected agent program
- `s`: `git add`, commit, and push the selected agent branch
- `x`: kill the selected agent, remove its clean worktree, and optionally delete its branch
- `?`: show help
- `q`: quit the cockpit without stopping agents

`--autoyes` is opt-in. When enabled, RobCo sends `y` + `Enter` to selected common
confirmation prompts that match its local waiting heuristic.

## Commands

```bash
robco debug
robco reset
```

`debug` prints the config, state, worktree, and resolved program paths. `reset` removes
RobCo's persisted state file but does not kill tmux sessions or delete worktrees.

## Release

RobCo follows the same local release model as dropr:

```bash
task release:dry
task release
```

The release pipeline builds the four supported targets, publishes archives to
`nantokaworks/robco-releases`, and updates `Formula/robco.rb` in
`nantokaworks/homebrew-tap`.

## Profiles

RobCo reads `~/.robco/config.json`. `default_program` can be either a shell command or a
profile name.

```json
{
  "default_program": "codex",
  "profiles": [
    { "name": "claude", "program": "claude" },
    { "name": "codex", "program": "codex" },
    { "name": "aider", "program": "aider --model ollama_chat/gemma3:1b" }
  ]
}
```

## dropr Overlay

RobCo is local-first and does not depend on dropr. When the `dropr` CLI is available and
authenticated, RobCo best-effort reads `dropr workspace list`, maps repository remotes to
dropr workspaces, and displays matching workspace metadata in the repo preview. Use
`--no-dropr` to disable this read-only overlay.
