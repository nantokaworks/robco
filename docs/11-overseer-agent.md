# 11 — Overseer Agent

Overseer is RobCo's local autonomous control system for dispatching ready dropr tasks,
supervising their workers, triaging exceptions, and optionally merging protected pull
requests. It is deliberately split so that polling and policy stay deterministic while
LLMs are used only for bounded judgment calls.

## Architecture

### Control plane

`robco overseer run` starts a single crash-only daemon guarded by
`~/.robco/overseer/overseer.pid`. On startup it loads the ledger and adopts any registered
RobCo agents whose parent id is `overseer` but which are missing from the ledger. Every
poll then performs the same ordered pass:

1. Reload `~/.robco/config.json`, update the Discord bot, and apply queued Discord
   ledger requests.
2. Gather new inbox reports, registry/tmux state, dropr task state, and GitHub pull
   request state. The observation is appended to
   `~/.robco/overseer/observations.jsonl`.
3. Reconcile those facts with `~/.robco/overseer/ledger.json`. The monitor advances
   phases, detects dead or stuck workers, escalates released task locks, and cleans up
   merged workers.
4. Queue and poll exception triage, execute deterministic monitor actions, run the
   auto-merge gate, and dispatch eligible ready tasks.
5. Atomically save the ledger, acknowledge completed triage and inbox work, write the
   heartbeat, and wait for the remainder of `poll_interval_secs`.

The dispatch engine considers ready tasks from dropr workspaces associated with RobCo's
registered repository remotes. It applies the dispatch toggle, daily limit, failure
circuit, skip list, retry limit, task-author filter, global worker limit, per-repository
limit, and a one-new-worker-per-repository-per-pass rule. Every decision is appended to
`~/.robco/overseer/decisions.jsonl`.

The auto-merge gate only considers ledger entries in `pr_opened`. It verifies main
branch protection, requires an open PR with a non-empty check rollup in which every
check is `SUCCESS`, and invokes `gh pr merge` with the configured strategy. Workers are
never instructed to merge their own pull requests.

### Execution plane

Each dispatched task gets one RobCo worker: one git worktree, one branch, and one tmux
session registered with parent id `overseer`. The worker receives an assignment prompt
that requires it to claim only its assigned dropr task, report lifecycle changes,
commit and push its branch, and open (but not merge) a pull request.

Overseer selects `worker_profile` when configured, otherwise `default_program`, and passes
that profile's `autonomous_args`. With the built-in profiles these are the client's
unattended permission flags. A non-empty autonomous argument list also activates two
spawn-time protections:

- Environment names matching `worker_env_blocklist` are set to empty in the worker's
  tmux environment.
- Claude workers receive local Stop and Notification hooks for `turn-done` and
  `waiting`; Codex workers receive a local `notify` command for `turn-done`.

Workers report to the append-only inbox at `~/.robco/overseer/inbox.jsonl`; the daemon
reads complete JSONL records, commits an offset only after the poll is persisted, and
rotates consumed data after the file reaches 1 MiB.

### Judgment plane

Overseer uses short-lived LLM processes for exception triage and conversational Discord
operations. These sessions are not the daemon and do not own the ledger.

Exception triage handles one queued failed or escalated worker at a time under
`~/.robco/overseer/triage/`. Discord operations sessions run under
`~/.robco/overseer/discord-ops/`. Both use the selected triage profile, receive a generated
`briefing.md`, and must write a schema-checked `result.json` before the configured
timeout. The process is then terminated. Returned actions are parsed into a closed
command enum; unknown or unsafe actions are rejected.

Task text, exception reasons, tmux capture, Discord messages, and other external values
are each placed in explicit `EXTERNAL_DATA` delimiters. Closing delimiter text inside a
value is escaped. The briefing tells the LLM to treat every such field as data, not as
instructions. Discord-generated impactful actions still pass through the same human
confirmation gate as typed commands.

## Configuration reference

Overseer configuration is the `overseer` object in `~/.robco/config.json`. Omitted fields use
these defaults:

```json
{
  "overseer": {
    "enabled": false,
    "dispatch_enabled": true,
    "auto_merge": false,
    "merge_strategy": "squash",
    "worker_profile": null,
    "max_workers": 3,
    "per_repo_limit": 1,
    "poll_interval_secs": 60,
    "stuck_after_mins": 30,
    "max_retries_per_task": 1,
    "daily_dispatch_limit": 20,
    "failure_circuit_threshold": 3,
    "triage_enabled": true,
    "triage_profile": null,
    "triage_timeout_mins": 15,
    "worker_env_blocklist": ["AWS_*", "*_TOKEN", "*_SECRET", "*_API_KEY"],
    "dispatch_task_authors": [],
    "discord": {
      "enabled": false,
      "token_env": "ROBCO_DISCORD_TOKEN",
      "channel_id": null,
      "allowed_user_ids": [],
      "notify_escalation": true,
      "notify_pr_opened": true,
      "notify_merged": true,
      "notify_circuit": true,
      "notify_worker_blocked": true,
      "action_limit_per_hour": 30,
      "confirmation_ttl_secs": 120
    }
  }
}
```

### Overseer fields

| Key | Type | Default | Implemented behavior |
|-----|------|---------|----------------------|
| `enabled` | boolean | `false` | Reported by status surfaces. It does not currently gate `robco overseer run` or the poll loop. |
| `dispatch_enabled` | boolean | `true` | Allows new dispatches. The circuit breaker and panic command persist this as `false`. |
| `auto_merge` | boolean | `false` | Enables the protected-branch and green-check auto-merge pass. |
| `merge_strategy` | string | `"squash"` | `"merge"` maps to `--merge`, `"rebase"` to `--rebase`, and every other value to `--squash`. |
| `worker_profile` | string or `null` | `null` | Profile name used for workers; `null` uses `default_program`. A missing profile supplies no autonomous arguments. |
| `max_workers` | non-negative integer | `3` | Maximum active non-terminal Overseer ledger entries globally. |
| `per_repo_limit` | non-negative integer | `1` | Maximum active Overseer ledger entries per repository. |
| `poll_interval_secs` | non-negative integer | `60` | Target period between daemon passes; also defines heartbeat freshness as `max(2 × value, 5)` seconds. |
| `stuck_after_mins` | non-negative integer | `30` | A dispatched, claimed, or working worker with older tmux activity is failed. |
| `max_retries_per_task` | non-negative integer | `1` | Dispatch is skipped when the highest recorded retry count reaches this value. |
| `daily_dispatch_limit` | non-negative integer | `20` | Maximum new workers recorded for the current UTC date. |
| `failure_circuit_threshold` | non-negative integer | `3` | Accumulated monitor or spawn failures that open the circuit and disable dispatch. The counter resets when a worker's PR merges or when an operator re-enables dispatch; a successful spawn alone does not reset it. |
| `triage_enabled` | boolean | `true` | Enables exception queueing and ephemeral triage sessions. |
| `triage_profile` | string or `null` | `null` | Profile used by triage and Discord ops; `null` uses `default_program`. |
| `triage_timeout_mins` | non-negative integer | `15` | Timeout for each triage or Discord ops LLM process. |
| `worker_env_blocklist` | array of strings | `["AWS_*", "*_TOKEN", "*_SECRET", "*_API_KEY"]` | Case-sensitive `*` globs for environment names neutralized in autonomous workers. |
| `dispatch_task_authors` | array of strings | `[]` | Exact allowlist for ready-task authors. Empty permits every author. |
| `discord` | object | see below | Discord gateway, command, and notification settings. |

### Discord fields

| Key | Type | Default | Implemented behavior |
|-----|------|---------|----------------------|
| `enabled` | boolean | `false` | Starts the supervised Discord gateway thread. |
| `token_env` | string | `"ROBCO_DISCORD_TOKEN"` | Name of the environment variable containing the bot token. The value is never read from config. |
| `channel_id` | string or `null` | `null` | Allowed parent channel id and notification destination. It must parse as a non-zero integer. |
| `allowed_user_ids` | array of strings | `[]` | Exact Discord user-id allowlist. The bot refuses to start when it is empty. |
| `notify_escalation` | boolean | `true` | Sends decision-log escalation notifications. |
| `notify_pr_opened` | boolean | `true` | Sends PR-opened daemon event notifications. |
| `notify_merged` | boolean | `true` | Sends merged daemon event notifications. |
| `notify_circuit` | boolean | `true` | Sends circuit-open notifications. |
| `notify_worker_blocked` | boolean | `true` | Sends worker-blocked daemon event notifications. |
| `action_limit_per_hour` | non-negative integer | `30` | Maximum mutating Discord actions in a rolling hour. Attempts count when execution begins. |
| `confirmation_ttl_secs` | non-negative integer | `120` | Lifetime of an impactful command's confirmation nonce. |

Profiles have an `autonomous_args` array. The built-in defaults are:

| Profile | `program` | `autonomous_args` |
|---------|-----------|-------------------|
| `claude` | `"claude"` | `["--dangerously-skip-permissions"]` |
| `codex` | `"codex"` | `["--dangerously-bypass-approvals-and-sandbox"]` |

For backward compatibility, an existing profile that omits `autonomous_args` gets an
empty array. Because worker hook injection and environment neutralization are currently
activated by a non-empty argument list, custom Overseer worker profiles should provide the
appropriate unattended argument explicitly.

## Security model

### Task trust boundary

Overseer workers execute task instructions and repository code on the local machine with
the selected profile's autonomous flags. Therefore, anyone who can author a task that
Overseer may dispatch can execute code with the worker process's operating-system access.
Use `dispatch_task_authors` as an exact task-author allowlist; leaving it empty accepts
all authors. The environment blocklist reduces accidental credential exposure but is
not a sandbox or a substitute for controlling task authors.

### Auto-merge prerequisite

Auto-merge requires GitHub `main` branch protection containing both required pull
request reviews and at least one required status check. Overseer verifies this through
`gh api repos/{owner}/{repo}/branches/main/protection`. Only successful verifications
are cached, for five minutes. Unprotected responses, command failures, non-zero exits,
and malformed JSON are not cached, so later poll passes retry them. A protected branch
is still held until the PR is open and every reported check is successful.

### Discord rails

Discord is disabled unless a token, a valid channel, and at least one allowed user are
configured. Messages outside the configured channel (or a Overseer-created exception
thread) and messages from users outside `allowed_user_ids` are ignored.

Impactful commands are not executed immediately. Overseer replies with a deterministic
description such as `Confirm: kill worker-x` and a random eight-character nonce. The
same user must reply `CONFIRM <nonce>` in the same channel before it expires. Dispatch
off and auto-merge off remain immediate; auto-merge on is never permitted through
Discord, including through the LLM ops agent. Mutating actions are rate-limited.

At gateway startup Overseer audits the application's requested install permissions and
warns if they exceed view/send/history/embed plus public-thread creation and management.
Once a command reaches execution, both success and failure are appended to the decision
log with Discord user attribution. Rate-limit refusals and rejected ops-agent actions
are audited too. Channel/user filtering is intentionally silent, and an invalid or
expired `CONFIRM` reply is rejected in-channel without a decision-log entry.

## Setup runbook

For first-time setup, run the interactive wizard from a terminal:

```sh
robco install
```

It probes `git`, `tmux`, `gh`, and `dropr`; offers MCP registration; walks through the
Overseer worker, triage, capacity, Discord, and macOS launchd settings; then writes
`~/.robco/config.json` once at the end. Existing values are prompt defaults, so a
second run that accepts every default leaves the configuration unchanged. Discord bot
tokens are never stored: the wizard records only `token_env` and, with explicit
confirmation on macOS, can copy that variable's current value into launchd.

This is a behavior change for bare `robco install`, which now requires a TTY and starts
the wizard. Automation and MCP-only setup must use `robco install --target
claude|codex|openclaw|all` or `robco install --all`. Bare `robco uninstall` is unchanged
and still targets every supported client.

### Prerequisites

Overseer expects `git`, `tmux`, `gh`, and `dropr` to be installed and authenticated. The
wizard treats missing `gh` or `dropr` as warnings and asks before continuing when
`git` or `tmux` is missing. Add the
repositories Overseer should manage to RobCo's registry, and ensure their remotes map to
accessible dropr workspaces. Configure at least one profile with suitable
`autonomous_args`.

Start locally with dispatch disabled while validating the installation:

```sh
robco overseer set dispatch off
robco overseer run
```

In another terminal:

```sh
robco overseer status
robco overseer set dispatch on
```

`robco overseer set auto-merge on|off` changes the merge toggle. These commands persist
their values in `~/.robco/config.json`.

### Discord application

1. In the Discord Developer Portal, create an application and add a bot.
2. Enable the privileged **MESSAGE CONTENT INTENT** for the bot. Overseer requests
   `GUILD_MESSAGES` and `MESSAGE_CONTENT` gateway intents.
3. Install the bot in the server with only View Channels, Send Messages, Send Messages
   in Threads, Create Public Threads, Manage Threads, Embed Links, and Read Message
   History.
4. Enable Developer Mode in Discord, then copy the operations channel id and each
   operator's user id into `channel_id` and `allowed_user_ids`.
5. Put only the environment variable name in `token_env`. Export the token separately:

   ```sh
   export ROBCO_DISCORD_TOKEN='replace-with-the-bot-token'
   ```

The token is read from the daemon's environment when the Discord thread starts; it is
never stored in `config.json`.

### launchd service on macOS

Install the service definition:

```sh
robco overseer install-service
```

This writes `~/Library/LaunchAgents/com.robco.overseer.plist` with `RunAtLoad` and
`KeepAlive`, but does not load it. If Discord is enabled, first make the already-exported
token available to the user's launchd environment, then load the service using the
exact path printed by the install command:

```sh
launchctl setenv ROBCO_DISCORD_TOKEN "$ROBCO_DISCORD_TOKEN"
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.robco.overseer.plist
```

Replace `ROBCO_DISCORD_TOKEN` in that command if `token_env` uses another name.

Inspect it at any time with:

```sh
robco overseer status
```

For a foreground daemon, `robco overseer stop` sends `SIGTERM` and waits briefly. For the
installed KeepAlive service, unload it to stop it without an immediate relaunch:

```sh
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.robco.overseer.plist
```

Daemon stdout and stderr go to `~/.robco/overseer/overseer.log` under launchd.

## Failure and recovery semantics

Overseer is crash-only: it has no separate shutdown checkpoint. Ledger, queue, completion,
cursor, and inbox-offset updates are persisted during normal passes, and launchd
restarts an exited daemon when installed. A pid guard prevents two daemon instances.

On startup, a missing ledger becomes empty; a corrupt ledger is renamed to
`ledger.json.corrupt`, then Overseer starts with defaults and rebuilds entries for
registered children whose parent is `overseer`. This adoption can recover worker identity,
branch, repository, and creation time, but not the original dropr task id, so the agent
id and title are substituted. Only a later inbox record carrying a task id can heal
that field; the current public report command does not include one.

The JSONL decision log is the durable audit trail used by `robco overseer status`, the TUI
Overseer info pane, and Discord notifications. The daemon writes observation snapshots
separately so a failed probe becomes a logged skipped observation instead of invented
state.

### Worktree management in the TUI

Press `e` on a worktree to enroll it under Overseer ownership in Auto mode. Enrolled
worktrees can be switched between Auto and Manual with `g`; Manual workers remain owned
by Overseer but are skipped for automatic dispatch. Press `E` and confirm to exclude an
enrolled worktree. Exclusion only detaches Overseer ownership and leaves the worker and
its tmux session running; use the separate kill action when the worker should also stop.

The triage queue is atomically persisted. At startup pending cases are loaded; an
unreadable queue is moved aside as `queue.json.corrupt`, logged, and restarted empty.
Each normalized triage completion is first written to `outcome.json`. If Overseer crashes
before acknowledging the queue item, the next run replays that marker as an idempotent
ledger update without executing the requested action again.

Use the local or Discord `!panic` kill switch during an incident:

```sh
robco overseer panic
```

It persists `dispatch_enabled: false`, kills every registered agent whose parent is
`overseer`, and audits the action. It does not stop the daemon or delete worktrees. The
failure circuit also persists dispatch off when accumulated failures reach
`failure_circuit_threshold`; the counter resets only when a worker's PR merges,
not on a merely successful spawn. An operator resets the circuit by running
`robco overseer set dispatch on`, which also resets the failure counter.
