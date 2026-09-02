# 09 — Config Reference

RobCo reads its configuration from `~/.robco/config.json`. The file is plain JSON, so
**comments are not allowed** — this page is the annotated reference instead. If the file
does not exist, RobCo runs entirely on the defaults documented below; the very first run
does not require any config at all.

## Full example (every key, all at their defaults)

```json
{
  "default_program": "claude",
  "hosts": [],
  "profiles": [],
  "branch_prefix": null,
  "worktree_root": "~/.robco/worktrees",
  "repos_root": "~/.robco/repos",
  "tmux_session_prefix": "robco_",
  "poll_interval_ms": 750,
  "dropr_overlay": true,
  "auto_accept": false,
  "process_indicator": true,
  "subagent_indicator": true,
  "merge_strategy": "squash",
  "pr_prompt": "Commit any remaining changes, push the branch, and open a pull request against main following the project's PR conventions.",
  "language": null,
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
| `hosts` | array of `{ssh, name?}` | `[]` | Remote RobCo installations shown after local repositories in the TUI. See [Remote hosts](#remote-hosts). |
| `profiles` | array of `{name, program}` | `[]` | Named launch commands. When `default_program` matches a profile `name`, that profile's `program` is executed. See [Profiles](#profiles). |
| `branch_prefix` | string or `null` | `null` | Prefix for branches RobCo creates. When `null`, it is derived as `<sanitized-repo-name>/` (e.g. `my.repo` → `my-repo/`). Set explicitly (e.g. `"robco/"`) to override. |
| `worktree_root` | string (path) | `"~/.robco/worktrees"` | Directory under which git worktrees are created. A leading `~` is expanded to your home directory. |
| `repos_root` | string (path) | `"~/.robco/repos"` | Persistent managed repository directory shared by the TUI, CLI, and Overseer daemon. A leading `~` is expanded on load. |
| `tmux_session_prefix` | string | `"robco_"` | Prefix applied to every tmux session name RobCo creates. |
| `poll_interval_ms` | integer (ms) | `750` | How often RobCo re-polls tmux for each agent's preview and status. Lower = snappier and more CPU; higher = calmer. |
| `dropr_overlay` | boolean | `true` | When `true`, best-effort reads `dropr workspace list` and shows matching workspace metadata in the repo preview. Reloaded at most once a minute (and whenever the repository set changes), so a linkage changed outside RobCo appears without a restart. Read-only; disable with this flag or `--no-dropr`. |
| `auto_accept` | boolean | `false` | When `true`, any agent detected as `Waiting` (awaiting a yes/no prompt) is auto-answered by sending `y` + Enter, throttled to at most once every 5 seconds per agent. |
| `process_indicator` | boolean | `true` | Enables `⚙ <cmd>` child-process detection. When enabled, each UI poll takes one system-wide `ps` snapshot shared by all rows; `false` skips that call and hides the indicator. |
| `subagent_indicator` | boolean | `true` | Enables passive Claude Code session reads, the `✻N` counts, and subagent details in the agent INFO pane. `false` skips the periodic `~/.claude/projects` filesystem reads and clears cached subagent activity. |
| `merge_strategy` | enum | `"squash"` | Strategy passed to `gh pr merge` when landing an agent's branch, by the TUI and the Overseer daemon alike. See [merge_strategy](#merge_strategy). |
| `pr_prompt` | string | `"Commit any remaining changes, push the branch, and open a pull request against main following the project's PR conventions."` | Prompt sent to the selected agent when `p` is confirmed. |
| `language` | string or `null` | `null` | Language every LLM surface is told to write its human-readable prose in. When `null`, RobCo sends the prompts it always has. See [language](#language). |
| `notify` | object | (all `true`) | Desktop-notification toggles per status. See [notify](#notify). |
| `project_icon` | enum | `"none"` | Marker style for the PROJECTS tree rows. See [project_icon](#project_icon). |

## Remote hosts

Each entry opens an independent `ssh <destination> robco mcp-stdio` connection. `ssh` is
passed through as one destination argument without parsing. `name` is optional tree chrome;
when omitted, the destination itself labels the host. Each host has a connection-state chip
in the PROJECTS header, and its repository rows appear after local repositories with an
`@name` suffix instead of a separate host group.

```json
{
  "hosts": [
    { "ssh": "prod", "name": "Production" },
    { "ssh": "operator@staging" }
  ]
}
```

`--host <destination>` is repeatable and adds ad-hoc destinations for one run. CLI values
are added to configured hosts and deduplicated by exact `ssh` destination string:

```sh
robco --host prod --host operator@staging
```

The legacy `ROBCO_REMOTE_HOST` variable behaves as one more ad-hoc destination. With no
configured, CLI, or environment hosts, the tree and local polling path are unchanged.

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

Controls the flag passed to `gh pr merge` when a branch is landed. This is the only merge
strategy setting: the TUI's `m` key and the Overseer daemon's auto-merge gate both read it,
so a config that sets it once cannot make the two paths disagree.

| Value | `gh` flag |
|-------|-----------|
| `"rebase"` | `--rebase` |
| `"squash"` (default) | `--squash` |
| `"merge"` | `--merge` |

`"rebase"` is the one value GitHub can refuse on a pull request it otherwise reports as
mergeable: a head branch carrying a merge commit cannot be replayed onto the base. RobCo
never substitutes another strategy for you — it names that cause in the merge banner and,
in the daemon, holds the entry under `merge_refused:rebase_refused_merge_commit`.

### Migrating from `overseer.merge_strategy`

Earlier versions carried a second key, `overseer.merge_strategy`, that only the daemon
read. It is retired. A config that still has it is migrated on load:

| Config on disk | Result |
|----------------|--------|
| Neither key | `"squash"` |
| `merge_strategy` only | That value |
| `overseer.merge_strategy` only | That value, adopted as `merge_strategy`, reported at startup |
| Both, same value | That value |
| Both, different values | The `overseer.merge_strategy` value wins, and the conflict is reported at startup |

The Overseer's value wins a conflict because it is the one the unattended merges have been
landing on: keeping it leaves the path nobody is watching working exactly as it was, and
moves the interactive path — where a failure is visible and retryable — onto it. The report
appears on the TUI's message banner and in the daemon log, and repeats at every start until
the config is rewritten. The retired key is dropped the next time RobCo writes the config.

The default moved with it. `merge_strategy` now defaults to `"squash"` — the Overseer's old
default — rather than `"rebase"`, so a config that named neither key no longer has the TUI
rebasing while the daemon squashes. The consequence is that a config naming only the
top-level key now applies it to the daemon as well, which is the point of the change: one
setting, both paths.

## language

Names the language every LLM surface RobCo drives is told to write its prose in. Write it
the way you would say it to a person — `"Japanese"`, `"日本語"`, and `"Brazilian Portuguese"`
are all valid; the value is handed to the model as natural language and nothing in RobCo
branches on it.

```json
{ "language": "Japanese" }
```

When the key is absent or `null`, RobCo sends exactly the prompts it always has.

### What it covers

Every long-form string an LLM writes for you to read:

| Surface | Field it governs |
|---------|------------------|
| Overseer board review | the review `summary` and each finding's `summary`, which become Inbox rows |
| Exception triage | the triage `reason` |
| Discord ops agent | the `reply` posted back to the channel |
| Worker dispatch and merge-recovery prompts | the prose a dispatched worker is instructed in |

The instruction is appended outside every `EXTERNAL_DATA` fence, so it is never mistaken
for the untrusted data those fences quarantine. A value that carries the fence marker is
refused and the key behaves as if unset; values are also trimmed, stripped of control
characters, and capped in length.

### What it does not cover

RobCo's own Rust strings — CLI help text, TUI labels and hints, log lines, and the
deterministic rule strings the Overseer's gates produce — stay in English. This is
deliberate, not an oversight: many of those strings are simultaneously machine keys. A
halt reason such as `merge_state:dirty` is persisted into the ledger as a deduplication
key and exact-matched by the merge-recovery classifier, so translating it would break
classification and break deduplication the moment the language changed. Model output is
free-form prose and carries no such contract, which is why the boundary sits there.

## overseer.worker_prompt_template

Task-specific text inserted into the prompt every dispatched worker receives — whether
dispatched by `!run <task>` from Discord or MCP, or by launching a dropr task row from the
repository INFO pane in the TUI. Both paths share one dispatch gate and one prompt
template; this key only ever changes the task-specific part of it.

```json
{
  "overseer": {
    "worker_prompt_template": "Task {display_id} ({task_id}): {title}\nRepository: {repo}\nSubtasks: {subtasks}\n\nFollow this project's CONTRIBUTING.md style guide."
  }
}
```

When the key is absent, `null`, or blank, RobCo uses its built-in task-specific text —
byte-for-byte what shipped before this key existed.

### Placeholders

| Placeholder | Value |
|-------------|-------|
| `{display_id}` | The task's display id, e.g. `#470`. |
| `{task_id}` | The task's dropr nanoid. |
| `{title}` | The task's title. |
| `{repo}` | The repository path the worker is dispatched into. |
| `{subtasks}` | Comma-separated display ids of the task's subtasks, or `none`. |

### What it covers

Only the task-specific instructions: how to work, what to check, house style — the part of
the prompt that names the task and tells the worker anything project-specific beyond the
built-in discipline. This is the same text a childless task and a parent task with
subtasks both start their prompt with.

### What it does not cover

The prompt's non-negotiable half is never reachable from this key, no matter what the
configured text says: the claim instruction (verify the Overseer's own claim before
touching the repository), the "open a pull request, do not merge it" ending, and the
rails — *never merge, never force push, never push to main, never change the shared
checkout's branch*. The code always appends these after the configured (or built-in)
task-specific text, so an override cannot delete or contradict them. This is deliberate:
the design this whole system rests on is that the machine never merges — only a person
does — and a config key that could remove that guarantee would undo it.

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
