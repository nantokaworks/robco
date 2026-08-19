# RobCo

RobCo is a repo-oriented terminal cockpit for supervising terminal AI agents across
multiple repositories. It discovers git repositories under a launch directory, shows a
`repo -> agents` tree, and runs each agent in its own git worktree and tmux session.

The full product specification and configuration reference are published at
<https://nantokaworks.github.io/robco/> (source under [docs/](docs/README.md)).

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

Run the interactive setup after installing the binary:

```bash
robco install
```

The retro-styled wizard checks prerequisites, optionally registers the MCP server,
configures Overseer and Discord, and can install the macOS launchd service. Bare
`robco install` now starts this wizard. For the previous non-interactive MCP-only
behavior, specify `--target claude|codex|openclaw|all` or use `--all`:

```bash
robco install --target codex
robco install --all
```

Bare `robco uninstall` continues to remove RobCo from every supported client.

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
- `g`: cycle the selected worktree through Overseer management (unmanaged → auto → manual)
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

## Overseer

Overseer is RobCo's local autonomous control system: it launches an operator-named dropr
task (via Discord's `!run`, MCP, or the TUI) into an isolated RobCo worktree/tmux worker,
reconciles its progress, triages failures, and can merge protected pull requests after
checks pass. Overseer never picks its own work — the operator always names the task. Read
the full architecture, configuration, security, and operations guide in
[docs/11-overseer-agent.md](docs/11-overseer-agent.md).

The recommended first-time setup is `robco install`, whose wizard preserves existing
values when Enter accepts each default and saves the Overseer configuration once at the
end. Tokens are referenced by environment-variable name and are never stored.

Start the daemon and inspect it — there is nothing to enable first:

```bash
robco overseer run
# In another terminal:
robco overseer status
```

On macOS, `robco overseer install-service` writes a launchd plist; run the
`launchctl bootstrap` command it prints to load the service.

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
