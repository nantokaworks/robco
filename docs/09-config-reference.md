# 09 — Config Reference

RobCo reads its configuration from `~/.robco/config.json`. The file is plain JSON, so
**comments are not allowed** — this page is the annotated reference instead. If the file
does not exist, RobCo runs entirely on the defaults documented below; the very first run
does not require any config at all.

## Full example (every key, all at their defaults)

```json
{
  "default_program": "claude",
  "profiles": [],
  "branch_prefix": null,
  "worktree_root": "~/.robco/worktrees",
  "tmux_session_prefix": "robco_",
  "poll_interval_ms": 750,
  "dropr_overlay": true,
  "auto_accept": false,
  "process_indicator": true,
  "subagent_indicator": true,
  "merge_strategy": "rebase",
  "pr_prompt": "Commit any remaining changes, push the branch, and open a pull request against main following the project's PR conventions.",
  "notify": {
    "enabled": true,
    "waiting": true,
    "idle": true,
    "done": true,
    "dead": true
  },
  "project_icon": "none"
}
```

You only need to include the keys you want to change; any key you omit falls back to its
default.

## Fields

| Key | Type | Default | What it does |
|-----|------|---------|--------------|
| `default_program` | string | `"claude"` | Program launched for each new agent. Either a **profile name** (resolved through `profiles`) or a raw shell command run as-is. |
| `profiles` | array of `{name, program}` | `[]` | Named launch commands. When `default_program` matches a profile `name`, that profile's `program` is executed. See [Profiles](#profiles). |
| `branch_prefix` | string or `null` | `null` | Prefix for branches RobCo creates. When `null`, it is derived as `<sanitized-repo-name>/` (e.g. `my.repo` → `my-repo/`). Set explicitly (e.g. `"robco/"`) to override. |
| `worktree_root` | string (path) | `"~/.robco/worktrees"` | Directory under which git worktrees are created. A leading `~` is expanded to your home directory. |
| `tmux_session_prefix` | string | `"robco_"` | Prefix applied to every tmux session name RobCo creates. |
| `poll_interval_ms` | integer (ms) | `750` | How often RobCo re-polls tmux for each agent's preview and status. Lower = snappier and more CPU; higher = calmer. |
| `dropr_overlay` | boolean | `true` | When `true`, best-effort reads `dropr workspace list` and shows matching workspace metadata in the repo preview. Read-only; disable with this flag or `--no-dropr`. |
| `auto_accept` | boolean | `false` | When `true`, any agent detected as `Waiting` (awaiting a yes/no prompt) is auto-answered by sending `y` + Enter, throttled to at most once every 5 seconds per agent. |
| `process_indicator` | boolean | `true` | Enables `⚙ <cmd>` child-process detection. When enabled, each UI poll takes one system-wide `ps` snapshot shared by all rows; `false` skips that call and hides the indicator. |
| `subagent_indicator` | boolean | `true` | Enables passive Claude Code session reads, the `✻N` counts, and subagent details in the agent INFO pane. `false` skips the periodic `~/.claude/projects` filesystem reads and clears cached subagent activity. |
| `merge_strategy` | enum | `"rebase"` | Strategy passed to `gh pr merge` when landing an agent's branch. See [merge_strategy](#merge_strategy). |
| `pr_prompt` | string | `"Commit any remaining changes, push the branch, and open a pull request against main following the project's PR conventions."` | Prompt sent to the selected agent when `p` is confirmed. |
| `notify` | object | (all `true`) | Desktop-notification toggles per status. See [notify](#notify). |
| `project_icon` | enum | `"none"` | Marker style for the PROJECTS tree rows. See [project_icon](#project_icon). |

## Profiles

`default_program` can be either a raw command or the `name` of a profile. Profiles let you
keep several agents configured and switch the default by name instead of retyping a
command. When a profile with a matching `name` exists, its `program` (which may include
arguments) is launched; otherwise `default_program` is executed verbatim.

```json
{
  "default_program": "codex",
  "profiles": [
    { "name": "claude", "program": "claude" },
    { "name": "codex", "program": "codex --ask-for-approval never" },
    { "name": "aider", "program": "aider --model ollama_chat/gemma3:1b" }
  ]
}
```

The example above makes **Codex** the default agent while keeping Claude and Aider
available as named profiles. To switch the default back to Claude, change
`default_program` to `"claude"`.

You can also override the program for a single run from the command line without editing
the config:

```sh
robco --program "codex --ask-for-approval never"
```

## merge_strategy

Controls the flag passed to `gh pr merge` when a branch is landed.

| Value | `gh` flag |
|-------|-----------|
| `"rebase"` (default) | `--rebase` |
| `"squash"` | `--squash` |
| `"merge"` | `--merge` |

## notify

Desktop notifications are opt-out per status. `enabled` is the master switch — when it is
`false`, no notifications fire regardless of the other flags.

| Key | Default | Fires when… |
|-----|---------|-------------|
| `enabled` | `true` | Master switch for all notifications. |
| `waiting` | `true` | An agent enters `Waiting` (awaiting your input). |
| `idle` | `true` | An agent goes `Idle`. |
| `done` | `true` | An agent finishes a turn (`Done`). |
| `dead` | `true` | An agent's tmux session is gone (`Dead`). |

Statuses `Running` and `BranchOnly` never notify. Worktree loss is tracked
independently as a `worktree_missing` flag over the agent's normal live status and
shown with a red `⌦` overlay. A worktree-gone notification fires once when the flag
changes from false to true.

## project_icon

Marker shown at the start of each PROJECTS row, reflecting collapsed/expanded state.

| Value | Collapsed | Expanded | Notes |
|-------|-----------|----------|-------|
| `"none"` (default) | `▸` | `▾` | Plain triangles; works in any terminal. |
| `"nerdfont"` | closed-folder glyph | open-folder glyph | Nerd Font folder icons (`nf-fa-folder` / `nf-fa-folder_open`); requires a patched Nerd Font. |
| `"emoji"` | 📁 | 📂 | Emoji folders. |

## Command-line overrides

A few config values can be overridden per invocation. CLI flags win over the file for that
run only; they are not persisted back to `config.json`.

| Flag | Overrides |
|------|-----------|
| `--program <cmd>` | `default_program` |
| `-y`, `--autoyes` | sets `auto_accept` to `true` |
| `--no-dropr` | sets `dropr_overlay` to `false` |

## Inspecting the resolved config

`robco debug` prints the resolved config path, state path, worktree root, the fully
resolved program command, and the effective `dropr_overlay` / `auto_accept` values — handy
for confirming which profile a name resolved to.
