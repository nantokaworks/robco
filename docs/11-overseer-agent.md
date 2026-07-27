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

1. Reload `~/.robco/config.json`, update the Discord bot, apply queued Discord
   ledger requests, and drain the runtime requests other processes left in
   `~/.robco/overseer/runtime_requests/`.
2. Gather new inbox reports, registry/tmux state, dropr task state, and GitHub pull
   request state. Pull request state is read for every entry that is still live, and
   also for an escalated entry that already has a pull request — an escalation is a
   question put to an operator, and the answer is often the merge itself. The
   observation is appended to `~/.robco/overseer/observations.jsonl`.
3. Reconcile those facts with `~/.robco/overseer/ledger.json`. The monitor drops entries
   for workers that are no longer Overseer children, advances phases, detects dead or
   stuck workers, escalates released task locks, and cleans up merged workers.
4. Queue and poll exception triage, execute deterministic monitor actions, run the
   auto-merge gate, and dispatch eligible ready tasks.
5. Drop the settled ledger entries that fall outside the retention window, then
   atomically save the ledger, acknowledge completed triage and inbox work, write the
   heartbeat, and wait for the remainder of `poll_interval_secs`.

That wait also ends early when another process enqueues a runtime request: enqueuing
signals the pid in the daemon pidfile, so an operator toggle such as
`robco overseer set dispatch on`, a panic stop, or a merge performed in the TUI starts a
pass within a couple of seconds rather than at the next tick. Passes are still kept at
least two seconds apart, so a burst of requests coalesces into one pass — every request
in the queue is drained by it — and the hold is recorded in `decisions.jsonl`. A woken
pass does exactly what a timed pass does; the request only decides *when*, never *what*.
Merges performed outside RobCo (GitHub's web UI, `gh` on another machine) have nothing
local to announce them and are still found by polling — including a merge an operator
performs to resolve an escalation, which is why an escalated entry with a pull request
is still read. That read only ever moves such an entry on a merge: a pull request that is
still open, or closed without merging, leaves it escalated and says nothing.

The dispatch engine considers ready tasks from dropr workspaces associated with RobCo's
registered repository remotes. It applies the dispatch toggle, daily limit, failure
circuit, skip list, retry limit, task-author filter, global worker limit, per-repository
limit, and a one-new-worker-per-repository-per-pass rule. A task whose ledger entry is
still in a non-terminal phase is held with reason `active_worker` whatever management
mode owns that worker, because the live worker still holds the task's branch and
worktree. Every decision is appended to `~/.robco/overseer/decisions.jsonl`.

Candidates are ordered before those gates run, by dropr task priority (`high`, `medium`,
`low`, then anything else) and then by ascending display id, which dropr assigns in
creation order — so the oldest task of the highest priority wins the last free worker
slot rather than whichever repository the registry happened to list first. The judge, when
one runs, receives this same ordering as its input, so it reorders a defined baseline.

A dispatch pass consults the LLM judge only when it is contended — when the approved
candidates outnumber the worker slots still available (`max_workers` minus live Auto
workers, bounded by one new worker per repository). Otherwise approving everything is the
only verdict the judge's authority can produce, so the pass dispatches its own ordering
immediately instead of spending a call and a whole extra poll cycle waiting for the
verdict. With `judge_profile` unset no dispatch judgment is ever enqueued, at any
candidate count. Each spawn records which path it took — `worker spawned:judge_approved`,
`worker spawned:judge_bypassed_uncontended`, or `worker spawned:judge_unconfigured` — so
the log says which dispatches an LLM had a hand in. A cached verdict whose candidate set
changed underneath it is dropped with a `judgment_discarded:candidate_set_changed:<key>`
entry rather than vanishing.

Those gates only see workers this Overseer started, and the ready list they run against
is minutes old by the time a judgment comes back. So immediately before a worker is
spawned, Overseer re-reads the task in dropr and takes its claim itself:

- A task another agent holds is not dispatched; the pass records a hold with reason
  `claimed_elsewhere:<agent>`, and the OVERSEER frame lists it under `standing off`.
- A claim that has outlived `claim_ttl_minutes` is taken over, recorded as
  `claim_expired:<agent>` so the takeover is never silent.
- A claim dropr will not grant, or a state read that fails, holds the candidate
  (`claim_refused:<reason>`, `claim_unavailable`, `claim_unreadable`) rather than
  spawning on an unknown claim.

The worker inherits that claim instead of competing for it: its prompt names `overseer`
as the holder and forbids re-claiming. A spawn that fails hands the claim straight back
rather than parking the task for its full TTL.

The auto-merge gate only considers ledger entries in `pr_opened`. It reads the pull
request, verifies protection on the branch that pull request targets, requires an open PR
with a non-empty check rollup in which every check is satisfied, requires GitHub to report
the pull request as mergeable, and invokes `gh pr merge` with the configured strategy.
Merges are serialised per repository: once one pull request of a repository merges, the
rest of that repository is held with `repo_merged_this_pass` until the next pass, because
the merge advanced their base. Repositories remain independent of each other. Workers are
never instructed to merge their own pull requests.

### Execution plane

Each dispatched task gets one RobCo worker: one git worktree, one branch, and one tmux
session registered with parent id `overseer`. The worker receives an assignment prompt
that hands it the claim Overseer already holds on its assigned dropr task, and requires
it to verify that claim rather than take one, report lifecycle changes, commit and push
its branch, and open (but not merge) a pull request.

All three names are built from one slug that leads with the task's source and number —
dropr task `#295` becomes `dropr-295-<title>`, capped at 32 characters on a hyphen
boundary. Leading with the source keeps the origin of the number readable and leaves the
numbering space open for a second task source later. Existing workers keep the names they
were created with; the shape applies to newly dispatched ones.

Overseer selects `worker_profile` when configured, otherwise `default_program`, and passes
that profile's `autonomous_args`. With the built-in profiles these are the client's
unattended permission flags. A non-empty autonomous argument list also activates two
spawn-time protections:

- Environment names matching `worker_env_blocklist` are set to empty in the worker's
  tmux environment, except the names the session credential channel resolves — see
  [Session credentials](#session-credentials).
- Claude workers receive local Stop and Notification hooks for `turn-done` and
  `waiting`; Codex workers receive a local `notify` command for `turn-done`.

Workers report to the append-only inbox at `~/.robco/overseer/inbox.jsonl`; the daemon
reads complete JSONL records, commits an offset only after the poll is persisted, and
rotates consumed data after the file reaches 1 MiB.

### Post-merge cleanup

An entry that reaches `merged` is cleaned up while its registry row still exists. The
daemon kills the worker's tmux session first and only then touches the worktree, so a
session that refuses to die defers the rest of the cleanup to the next pass instead of
pulling a worktree out from under a live shell. The cleanup itself is the same sequence
the TUI merge action runs, and lives in one place so the two cannot drift: fast-forward
the primary worktree, remove the task worktree, delete the local branch, delete the
remote branch.

The two paths differ only in what a failing step means. The TUI merge is watched, so it
stops at the first failure and shows it. The daemon is not, so it logs the failure to
`~/.robco/overseer/decisions.jsonl` and runs the remaining steps — a `main` that cannot
fast-forward must not strand a worktree and a branch forever. The registry row is dropped
only once the worktree is actually gone, which is what makes a failed removal retry on
the next pass.

The local branch is deleted only when its changes are provably in the base. Ancestry is
not the test: under the default squash strategy the branch tip is not an ancestor of the
base, so the check compares patch ids as well, covering merge, rebase, and squash
landings alike. A branch that fails the check — including one whose base branch is stale
because the fast-forward failed — is left in place with the reason logged, never
force-deleted. Remote branch deletion stays best-effort: GitHub's own
auto-delete-branch setting usually gets there first, and its absence is not a failure.

### Judgment plane

Overseer uses short-lived LLM processes for exception triage and conversational Discord
operations. These sessions are not the daemon and do not own the ledger.

Exception triage handles one queued failed or escalated worker at a time under
`~/.robco/overseer/triage/`. Discord operations sessions run under
`~/.robco/overseer/discord-ops/`. Both use the selected triage profile, receive a generated
`briefing.md`, and must write a schema-checked `result.json` before the configured
timeout. The process is then terminated. Returned actions are parsed into a closed
command enum; unknown or unsafe actions are rejected.

The board review is the one surface that reads Overseer's own history rather than a single
case. It runs on its own clock (`review_interval_mins`), never on every poll. Its
deterministic stage runs whether or not a reviewer model is configured: `review_profile`
switches on the model stage alone, and detection that depended on it would be detection
that never ran on the default configuration — which is exactly what happened, leaving the
two rules below with no recorded finding across the daemon's whole history. Each run
builds a size-bounded digest —
at most 200 recent decisions with each reason truncated, at most 50 live ledger entries
with their age, and the dispatch counters — and applies deterministic rules to it: a
reason repeating three times or more is reported as `repeating_failure` (a structural
fault) or `repeating_hold` (nothing is moving), a live entry older than twice
`stuck_after_mins` as `stalled`, and the failure counter as `circuit_at_risk` or
`circuit_open` with the latest failure named. Those findings are escalation entries in
`decisions.jsonl` under source `review`, so they surface in the OVERSEER frame like any
other escalation; a finding that persists is escalated once, not once per interval. The
review's own entries are excluded from the next digest so it cannot diagnose itself.

The digest and those findings are then handed to a reviewer session under
`~/.robco/overseer/review/`, which is charged to `daily_review_budget` rather than
`daily_llm_budget` — at a 15-minute cadence a shared budget would starve dispatch and
merge judgement. Its result schema carries a severity and a sentence and nothing else:
`warn` and `critical` findings become escalations, `info` is recorded only, and there is
no field through which the reviewer can dispatch, merge, unblock, or write the ledger. An
exhausted budget stops the session but not the deterministic findings, and records
`review_budget_exhausted` so a quiet reviewer does not read as a healthy board. A missing
profile stops the session too, and records nothing: no budget is charged and no session is
spawned, because there is no model to run. `robco overseer status` reports the judge and
review counts separately, and names the two states apart — `findings every 20m, no
reviewer model` against `every 20m via <profile>` — so a quiet board can be read as
"nothing was found" rather than "nothing looked".

Task text, exception reasons, tmux capture, Discord messages, and other external values
are each placed in explicit `EXTERNAL_DATA` delimiters. Closing delimiter text inside a
value is escaped. The briefing tells the LLM to treat every such field as data, not as
instructions. Discord-generated impactful actions still pass through the same human
confirmation gate as typed commands.

## Configuration reference

Overseer configuration is the `overseer` object in `~/.robco/config.json`. Omitted fields use
these defaults.

One setting outside this object changes what the Overseer writes: the top-level
[`language`](09-config-reference.md#language) key. Every LLM surface the Overseer drives —
board review, exception triage, the dispatch and merge judges, the Discord ops agent, and
the prompts it hands to workers — is told to write its human-readable prose in that
language, so the review summaries and judge reasons that land in the Inbox come back in it.
The Overseer's own deterministic strings, including the halt reasons the merge-recovery
classifier matches on, stay in English. With the key unset the Overseer sends the prompts it
always has.

```json
{
  "overseer": {
    "dispatch_enabled": true,
    "auto_merge": false,
    "protection_mode": "required",
    "autonomy_level": "conservative",
    "max_branch_updates": 3,
    "merge_recovery_enabled": false,
    "max_merge_recoveries": 2,
    "max_merge_holds": 30,
    "worker_profile": null,
    "max_workers": 3,
    "per_repo_limit": 1,
    "terminal_retention_per_repo": 50,
    "poll_interval_secs": 60,
    "stuck_after_mins": 30,
    "max_retries_per_task": 1,
    "daily_dispatch_limit": 20,
    "failure_circuit_threshold": 3,
    "triage_enabled": true,
    "triage_profile": null,
    "review_profile": null,
    "review_interval_mins": 20,
    "daily_review_budget": 96,
    "triage_timeout_mins": 15,
    "worker_env_blocklist": ["AWS_*", "*_TOKEN", "*_SECRET", "*_API_KEY"],
    "session_env": {},
    "session_env_file": null,
    "session_preflight": true,
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
| `protection_mode` | `"required"`, `"relaxed"`, or `"off"` | `"required"` | How strictly the auto-merge gate requires the pull request's base branch to be protected. `required` demands both a pull-request requirement and at least one required status check; `relaxed` demands only the pull-request requirement; `off` skips the probe. Set it with `robco overseer protection <mode>`. |
| `autonomy_level` | `"approval_only"`, `"conservative"`, or `"full_auto"` | `"conservative"` | How much of the merge envelope the daemon may clear without an operator. `approval_only` escalates every merge; `conservative` auto-merges only a docs-or-tests change under 5 files and 200 lines that trips no risk; `full_auto` escalates just the hard stops — destructive changes, security-sensitive changes, repeated failures, an exhausted LLM budget, and external side effects. Set it with `robco overseer autonomy <level>`. |
| `merge_strategy` | — | — | Retired. The strategy is the top-level [`merge_strategy`](09-config-reference.md#merge_strategy), which the TUI reads too, so the two merge paths cannot disagree. A config still carrying this key is migrated on load and the key is dropped on the next write. |
| `max_branch_updates` | non-negative integer | `3` | Times the auto-merge gate may update one pull request's branch onto its base before escalating that entry. Each attempt is charged before it runs, so an update that fails still spends budget. `0` never updates a branch and escalates the first time one falls behind. |
| `merge_recovery_enabled` | boolean | `false` | Hands a merge failure the owning worker could fix back to that worker's live session instead of parking the pull request. Default-off, so a daemon that has never heard of merge recovery behaves exactly as it did before it existed. Switched off, each failure it would have acted on is still recorded once per revision as `merge_recovery_disabled:<reason>` and counted into `merge-recovery: off (N dropped)`. |
| `max_merge_recoveries` | non-negative integer | `2` | Handbacks one pull request may be charged before it escalates to an operator. Each attempt is charged before it runs, so a handback that never reaches its worker still spends budget. `0` never hands anything back and escalates the first recoverable failure. |
| `max_merge_holds` | non-negative integer | `30` | Auto-merge passes one pull request may be held under the same reason at the same head before the entry escalates with `merge_hold_cap_reached:<reason>`. Without it every non-merge exit re-records its reason once per poll for as long as the condition lasts. At the default `poll_interval_secs` the default is thirty minutes — past the 5-15 minutes a healthy check run takes, and well inside an hour. Exits with their own budget (`behind_*`, the settle barrier) are not charged twice. `0` escalates on the first held pass. |
| `worker_profile` | string or `null` | `null` | Profile name used for workers; `null` uses `default_program`. A missing profile supplies no autonomous arguments. |
| `max_workers` | non-negative integer | `3` | Maximum active non-terminal Overseer ledger entries globally. Manual entries count too — see below. |
| `per_repo_limit` | non-negative integer | `1` | Maximum active Overseer ledger entries per repository. Manual entries count too — see below. |
| `terminal_retention_per_repo` | non-negative integer | `50` | Settled (`merged`, `failed`, `escalated`) ledger entries kept per repository. The oldest beyond the window are dropped at the end of a pass — see below. `0` keeps every settled entry, which is how the ledger behaved before the window existed. |
| `poll_interval_secs` | non-negative integer | `60` | Target period between daemon passes; also defines heartbeat freshness as `max(2 × value, 5)` seconds. |
| `stuck_after_mins` | non-negative integer | `30` | A dispatched, claimed, or working worker with older tmux activity is failed. |
| `max_retries_per_task` | non-negative integer | `1` | Dispatch is skipped when the highest recorded retry count reaches this value. Every dispatch attempt for a task is recorded before its worker is spawned, so an attempt whose spawn fails counts too. The default permits one first attempt and one retry. |
| `daily_dispatch_limit` | non-negative integer | `20` | Maximum new workers recorded for the current UTC date. |
| `failure_circuit_threshold` | non-negative integer | `3` | Accumulated monitor or spawn failures that open the circuit and disable dispatch. The counter resets when a worker's PR merges or when an operator re-enables dispatch; a successful spawn alone does not reset it. |
| `triage_enabled` | boolean | `true` | Enables exception queueing and ephemeral triage sessions. |
| `triage_profile` | string or `null` | `null` | Profile used by triage and Discord ops; `null` uses `default_program`. |
| `review_profile` | string or `null` | `null` | Profile used by the periodic board review's model stage. `null` runs the pass without a model: the digest is still built and the deterministic findings still escalate, but no session is spawned and no review budget is charged. A named profile that does not exist fails the session rather than falling back. |
| `review_interval_mins` | non-negative integer | `20` | Minimum minutes between board reviews. The last run time is persisted, so a daemon that restarts often still reviews on this cadence rather than on every start-up. |
| `daily_review_budget` | non-negative integer | `96` | Board-review sessions per UTC date, counted separately from `daily_llm_budget`. Exhausting it stops the reviewer session, not the deterministic findings. |
| `triage_timeout_mins` | non-negative integer | `15` | Timeout for each triage, judgment, or board-review LLM process. |
| `worker_env_blocklist` | array of strings | `["AWS_*", "*_TOKEN", "*_SECRET", "*_API_KEY"]` | Case-sensitive `*` globs for environment names neutralized in autonomous workers. Names the session credential channel resolves are exempt — see [Session credentials](#session-credentials). |
| `session_env` | object of string → string | `{}` | Environment applied to every session the daemon spawns and to every agent robco launches (dispatched workers and TUI-created agents alike). Highest layer of the credential channel; also written into the launchd plist by the installer. See [Session credentials](#session-credentials). |
| `session_env_file` | string (path) or `null` | `null` | `KEY=VALUE` file read below `session_env`. `null` reads `~/.robco/env`. A leading `~` is expanded. Read at spawn time, so a rotated token needs no reinstall. |
| `session_preflight` | boolean | `true` | Spawns one probe session at daemon start to confirm the channel authenticates, and records the verdict for `robco overseer status`. |
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

## Session credentials

The Overseer spawns two different kinds of process, and until recently only one of them
could authenticate.

- **Workers** are launched into the tmux server, which belongs to the user's login session.
  They inherit that session's environment and its keychain access.
- **Ephemeral sessions** — exception triage, the dispatch and merge judges, the board
  reviewer, the Discord ops agent — are spawned as direct children of the daemon. Under the
  installed launchd service the daemon has no login session behind it, so those children
  cannot reach the macOS keychain item the Claude CLI keeps its OAuth credential in. Every
  such session failed in ~25 ms with `Failed to authenticate: OAuth session expired and
  could not be refreshed`, wrote no `result.json`, and the merge judge turned the silence
  into a fail-safe escalation.

A service daemon is not supposed to borrow an interactive login session's secret store. The
answer is the same one systemd (`Environment=` / `EnvironmentFile=`), the AWS CLI (`AWS_*`
before `~/.aws/credentials`), and `gh` (`GH_TOKEN` before `hosts.yml`) give: an explicit,
non-interactive channel with a documented resolution order.

### Resolution order

First hit wins, per variable name:

| # | Source | Notes |
|---|--------|-------|
| 1 | `overseer.session_env` in `~/.robco/config.json` | Explicit assignments. Also what the launchd installer materialises. |
| 2 | The env file — `overseer.session_env_file`, or `~/.robco/env` | `KEY=VALUE` lines; `#` comments, blank lines, a leading `export `, and one layer of matching quotes are all handled. |
| 3 | Whatever the daemon process inherited | Not an assignment — plain inheritance. A name nobody configured keeps the daemon's value. |

Layers 1 and 2 are applied to the spawned process; layer 3 is what happens to everything
else. Nothing here reads the keychain, by design: an agent cannot unlock the login session's
keychain, which is the failure the channel exists to route around.

### Setting it up

```sh
claude setup-token                       # prints a long-lived OAuth token
printf 'CLAUDE_CODE_OAUTH_TOKEN=%s\n' "$TOKEN" > ~/.robco/env
chmod 600 ~/.robco/env
robco overseer install-service           # rewrites the plist
launchctl bootout   gui/$(id -u) ~/Library/LaunchAgents/com.robco.overseer.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.robco.overseer.plist
robco overseer status                    # check the `session auth:` line
```

`ANTHROPIC_API_KEY` and `OPENAI_API_KEY` are recognised the same way; the three names are
what the health surface knows how to report, and any other name in the channel is still
passed through to the session.

### What the installer writes, and what it does not

`robco overseer install-service` writes `overseer.session_env` into the plist's
`EnvironmentVariables` dictionary, beside `PATH`, and then chmods the plist to `600`. It
does **not** copy the env file in. That split is deliberate:

- The plist is the only way to give the *daemon process itself* a variable, since a launchd
  agent inherits nothing — that is why `PATH` has always had to be materialised there.
- The env file is read by the daemon at spawn time, so rotating a token is one file write.
  Copying it into the plist would make every rotation a reinstall-and-reload, and would put
  a second copy of the secret in a second file.

The service is **not** wrapped in a login shell. Doing so would make the daemon's
environment depend on whatever the operator's shell profile happens to export, which is the
implicit inheritance this channel replaces.

### `worker_env_blocklist` interaction

`worker_env_blocklist` blanks environment names in autonomous workers, and its defaults
(`*_TOKEN`, `*_API_KEY`) match exactly the names a session credential goes by. **Every name
the channel resolves — from `session_env` or from the env file — is exempt from the
blocklist, and is set in the worker's tmux environment.** Names the channel does not carry
are still blanked exactly as before.

The decision turns on what the blocklist is for. It strips *ambient* credentials: names the
daemon happens to be carrying that an autonomous worker never asked for and cannot be
trusted with. The channel is the opposite — an operator writing down what robco's own
processes are supposed to run under — so the explicit declaration wins over the wildcard
default.

Not exempting it would break the recommended headless install in a way that is hard to see:
a worker dispatched by a launchd-owned daemon has the same problem the ephemeral sessions
do, and the one credential the operator configured for it would be blanked on the way in.
Both layers of the channel are operator-authored configuration, so the exemption covers both
rather than drawing a line between the config map and the file.

The consequence to be aware of: the env file is not a general-purpose secret store. Anything
in it reaches dispatched workers. Put only what the agents are meant to run under there.

### Health

With `session_preflight` on (the default) the daemon spawns one probe session at start-up
and records the verdict in `~/.robco/overseer/session_health.json`. Any live session that is
refused on credentials overwrites the same record. `robco overseer status` prints it:

```
session auth: ok (CLAUDE_CODE_OAUTH_TOKEN via session env file, checked 3m ago)
session auth: failed (no credential configured, checked 0m ago) — Failed to authenticate: OAuth session expired and could not be refreshed
```

A failed state also prints a warning naming the recovery. Each session's stderr is captured
to `session.log` in its case directory, which is where the detail comes from.

A judgment refused on credentials is recorded with the reason `session_auth_failed:` instead
of the generic `judgment fail-safe:` wording, so `~/.robco/overseer/decisions.jsonl` can be
grepped for the cause. It is still a fail-safe verdict — the pull request is escalated, not
merged — because a session that never ran has produced no opinion worth acting on.

## Security model

### Task trust boundary

Overseer workers execute task instructions and repository code on the local machine with
the selected profile's autonomous flags. Therefore, anyone who can author a task that
Overseer may dispatch can execute code with the worker process's operating-system access.
Use `dispatch_task_authors` as an exact task-author allowlist; leaving it empty accepts
all authors. The environment blocklist reduces accidental credential exposure but is
not a sandbox or a substitute for controlling task authors.

### Auto-merge prerequisite

Under the default `required` mode, auto-merge requires the pull request's base branch to
be protected by both a pull-request requirement and at least one required status check.
GitHub reports protection through two independent APIs, and Overseer probes both:

- `gh api repos/{owner}/{repo}/rules/branches/{branch}` returns the effective rules of
  every ruleset targeting the branch. A `pull_request` rule supplies the pull-request
  requirement and a `required_status_checks` rule with a non-empty context list supplies
  the check requirement.
- `gh api repos/{owner}/{repo}/branches/{branch}/protection` is the classic API. It
  answers `404 Branch not protected` for a repository protected only by rulesets, which
  is why the rulesets probe exists; conversely the rulesets endpoint never reports
  classic protection.

The facts from both sources are combined, because GitHub enforces them simultaneously.
Only verifications that satisfy the active mode are cached, for five minutes, keyed by
repository, branch, and mode — a loosened mode or a different base branch re-probes.
Command failures, non-zero exits, and malformed JSON are not cached, so later poll passes
retry them; a branch that answered no probe at all is held as
`unprotected:probe_unavailable` rather than treated as unprotected. A protected branch is
still held until the PR is open and every reported check is successful.

`relaxed` accepts a base branch that merely forces changes through pull requests, for
operators whose CI is not wired into required checks. `off` skips the probe entirely and
relies on GitHub's own mergeability signal. Neither is the default and no existing
configuration is migrated onto them. Both are recorded: every merge and every
protection hold carries the active mode in its `decisions.jsonl` entry
(`"protection_mode": "relaxed"`), so a merge that only happened because the gate was
loosened stays distinguishable from one that cleared full protection. A refusal names the
failing condition — `unprotected:no_pull_request_rule`,
`unprotected:no_required_status_checks`, `unprotected:probe_unavailable`, or
`unprotected:unknown_remote`.

### Behind branches and other merge states

Protection and green checks are Overseer's own conditions; whether GitHub will accept the
merge is a separate question it answers with `mergeStateStatus`. The gate reads that field
from the same `gh pr view` and acts on it before merging.

`BEHIND` is the state that matters most. Merging one pull request advances the base
branch, which leaves every other open pull request of that repository missing the new base
commit. A base branch whose ruleset sets `strict_required_status_checks_policy` refuses to
merge such a branch, so without intervention only the first pull request of each
repository ever lands. The gate therefore runs `gh pr update-branch` and returns the entry
to the queue under
`behind_branch_updated`. It does not merge in the same pass: the update creates a new head
whose required checks have not run yet, and the check rollup always describes the current
head, so the next pass holds with `checks_waiting` until they report. Falling behind is an
expected, recoverable state and never counts toward `consecutive_failures`.

The update is a merge commit from the base by default. Only when the top-level
[`merge_strategy`](09-config-reference.md#merge_strategy) is `rebase` does the gate pass
`--rebase`, which rewrites the pull request's own branch; no other branch is ever
rewritten. The two follow one setting deliberately: a branch updated with a merge commit is
exactly the shape GitHub later refuses to rebase-merge.

Each update is charged to the entry's `branch_updates` before it runs, and
`max_branch_updates` bounds it. A branch that keeps losing the race against other merges —
or one whose update keeps failing — escalates with `behind_update_cap_reached` instead of
looping, and `decisions.jsonl` also carries the failing update itself as
`behind_update_exit:<status>` or `behind_update_error:<error>`.

Every other non-mergeable state is held under its own name, so the log says why a pull
request is parked: `merge_state:dirty` needs the conflict with the base resolved,
`merge_state:blocked` is missing an approval or a required check, `merge_state:draft` is
not ready, and `merge_state:unknown` is GitHub still computing mergeability. A state
GitHub adds later is held under its own lowercased name. `CLEAN` and `HAS_HOOKS` proceed;
a pull request that reports no merge state at all proceeds too, since the rest of the gate
has already cleared it.

### How long a hold may last

Every gate exit that is not a merge names a reason and returns, and the next pass one poll
interval later reads the same pull request, reaches the same exit, and records the same
reason again. For a check run that finishes in ten minutes that is a running commentary;
for a condition nothing is going to clear it is the entire life of the pull request
written to `decisions.jsonl` one line at a time, with no counter, no age, and no phase
change — the board reads healthy while nothing moves.

So a held pass costs budget. Each one is charged to the entry's `merge_hold` and bounded by
`max_merge_holds`; the pair the budget is spent on is (reason, head sha), so a new head —
the worker's answer to the last one — and a changed reason each restart the count, while a
frozen pair keeps spending it. When the budget runs out the entry reaches `escalated`,
records `merge_hold_cap_reached:<reason>` once, and stops recording that hold, which is
what puts it in front of an operator in `robco overseer status` and the TUI Inbox instead
of leaving it to accumulate identical lines.

The exits that already carry a budget of their own are not charged here: the `behind_*`
family is bounded by `max_branch_updates`, the settle barrier by `max_merge_settle_passes`,
and a skip leaves the entry terminal already. One condition must never spend two budgets
and escalate under whichever ran out first.

An entry that gets past the deterministic gate — it merged, or only its judgment is
outstanding — forgets what it was held on, so a condition that comes back after clearing
starts from a full budget rather than inheriting the old one's residue.

### A pull request that has already settled

Not every pull request the gate reads is still open. The TUI merges, `gh` merges, and
merges from GitHub's web UI all land without telling Overseer, and an operator answering
an escalation usually answers it that way. The gate therefore reads the pull request's
own `state` from the same `gh pr view` and acts on it **before** the protection probe,
because everything below that point costs GitHub calls that cannot change the answer:

- `MERGED` takes the entry to `merged` under `pr_already_merged`, recorded as a skip
  rather than a hold — nothing failed, and the merge this entry was waiting for
  happened. The monitor cleans the worker up from there like any other merge.
- `CLOSED` without a merge stays escalated under `pr_closed_unmerged`. Nothing landed
  and no worker can make it land, since reopening a pull request is a human act, so it
  belongs in the operator inbox. `failed` would have the ledger report a failure no
  worker committed.

A state Overseer does not recognise, or a read that reports none at all, is not treated
as a conclusion: it still holds under `checks_waiting`, because a read that did not
answer is not a terminal fact.

Neither conclusion raises the per-repository settle barrier. That barrier guards against
reads made stale by a merge *this pass* performed, and it is lowered by the `git pull
--ff-only` in the cleanup sequence — which may never run for a worker that is already
gone. An external merge advanced the base long ago, so raising the barrier would park the
repository until `repo_merge_settle_cap_reached` for nothing.

Both conclusions also drop any terminal verdict the merge judge left for that pull
request. That verdict is what keeps an escalated entry re-entering the gate every pass to
be reconsidered, and a pull request that can never be merged again has nothing left to
re-judge. It is what holds the invariant this exit exists for: every exit taken for a
pull request that is no longer open leaves the reconsidering set, so the same decision is
recorded once instead of once per poll interval.

### Merge recovery

A merge failure is not always an operator's problem. The worker that wrote the branch is
still alive when the gate gives up — `reconcile_entry` only kills a worker's session and
removes its worktree once the entry reaches `merged` — and most of the reasons above name
something that worker could fix from inside its own worktree. With
`merge_recovery_enabled`, the gate hands those failures back to it instead of parking the
pull request until a human looks.

Overseer keeps sole possession of the merge throughout. The remediation prompt asks the
worker to fix the branch it was already assigned, push it, and report done; it restates
the `never merge / never force push / never push to main / never create extra worktrees`
rails, and the merge gate and merge judge remain the only path to a merge.

Reasons are classified into the worker's and the operator's:

- **Worker-fixable:** `merge_state:dirty`, `merge_state:blocked`, `checks_not_green`,
  `behind_update_cap_reached`, `merge_exit:<status>`, `merge_error:<error>`,
  `judge_veto:<reason>`, and `judge_escalate:<reason>`. The judge's reason is passed to the
  worker verbatim, because it is the actual instruction.
- **Operator-only:** `unprotected:*`, `missing_pr_url`, `autonomy_envelope`,
  `repo_merged_this_pass`, `behind_branch_updated` (already the recovery), `checks_waiting`
  (nothing has failed yet), `pr_already_merged` (the pull request landed, so there is
  nothing to fix), `pr_closed_unmerged` (only a human can reopen it), `merge_refused:*`,
  and every probe or parse failure. **An unrecognised reason is operator-only** — a failure
  nobody anticipated must not silently drive a worker.

`merge_refused:rebase_refused_merge_commit` is a rebase GitHub declined because the head
branch carries a merge commit. It is deliberately the operator's: the worker cannot clear
it without rewriting a published branch, which the rails forbid, and the fix is to choose
another [`merge_strategy`](09-config-reference.md#merge_strategy).

`checks_not_green` means a check finished and did not pass. A head whose checks have not
finished holds under `checks_waiting` instead, so a worker turn is never spent on a pull
request that has not failed at anything. A pull request that is no longer open never
reaches either: the gate concludes on it first.

The rollup is read per check *name*, on that name's most recent run, which is the unit
branch protection requires. Two consequences: a run superseded by a newer one of the same
name — a duplicate a workflow's concurrency group cancelled, or one stranded in the queue
behind the run that overtook it — cannot veto the run that replaced it, and a name whose
newest run genuinely failed still reads `checks_not_green`. Runs of one name that started
in the same second cannot be ordered, so the gate holds until both have reported.
`SKIPPED` and `NEUTRAL` are read as satisfied rather than as still-running: they are
terminal conclusions, and a required workflow a path filter excluded reports one of them
and will never report anything else.

Each handback is charged to the entry's `merge_recovery.charged` before it runs, bounded by
`max_merge_recoveries`, and deduplicated by the head sha it was charged against. The same
failure on the same revision is therefore handed back once rather than once per poll
interval; a new head resets that deduplication — the worker pushed, so the next failure is
a genuinely new one — but never the budget, or a worker that pushes a broken fix each round
would loop forever. A spent budget escalates with `merge_recovery_cap_reached`.

With `merge_recovery_enabled` off — the default — nothing is handed back, and the
classification above is inert. It is not silent, though: a failure that classified as
worker-fixable is recorded once per (entry, head sha) as
`merge_recovery_disabled:<reason>`, and `robco overseer status` reports the running total
beside the switch as `merge-recovery: off (N dropped)`. The entry keeps its phase and no
worker is touched, so the daemon behaves exactly as it did before; what changes is that
the setting now reads as a consequence rather than a flag. Operator-only failures record
nothing either way — they were never a worker's to fix. Whether to switch the setting on
stays the operator's call; this is the evidence for making it.

A handback returns the entry to `pr_opened` so the next pass re-evaluates it normally; a
judge veto had escalated it, and that escalation is superseded rather than left to strand
the pull request. Manual-managed workers are never handed back to, since `worker_is_auto`
already gates the whole merge pass. A worker that is no longer registered, or whose tmux
session is gone, escalates under
`merge_recovery_skipped:missing_session:<agent>` rather than being silently dropped.

`decisions.jsonl` carries the whole cycle under `source: "merge_recovery"`:
`merge_recovery_dispatched:<reason>` for each handback,
`merge_recovery_skipped:send_failed:<error>` when the prompt did not reach the session, and
`merge_recovery_cap_reached` when the budget runs out. Each handback also posts a scribble
on the dropr task; a scribble that fails to land is logged and does not abort the merge
pass. `robco overseer status` and the TUI OVERSEER frame both report the switch and its cap.

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

`robco overseer set auto-merge on|off` changes the merge toggle, and
`robco overseer protection required|relaxed|off` changes how strictly that gate requires
base-branch protection, and `robco overseer autonomy approval_only|conservative|full_auto`
changes how much of the merge envelope the daemon clears on its own. These commands persist
their values in `~/.robco/config.json`. `robco overseer status` and the TUI OVERSEER frame
report the active protection mode and autonomy level next to `auto-merge`, and both warn
while auto-merge runs under a loosened gate — naming, for `full_auto`, the risks the
envelope stops escalating.

### Which build the daemon is running

The daemon executes the image it started from until the service restarts, so a fix that
is merged, released, and installed does not reach the board until the daemon is restarted
too. Each pass therefore records its own version in the heartbeat, and
`robco overseer status` reports it as `version=` beside `pid` and `heartbeat`. When that
version differs from the `robco` binary answering the command — the exact
"installed but not restarted" state — both the status command and the TUI Health frame
warn and name the two builds; the OVERSEER header carries it as a `stale build` warning
row. A heartbeat written before the daemon recorded its build reads as `unknown` and
warns the same way, because only a release older than this one leaves the field out.
Restart the daemon (`robco overseer stop` then `robco overseer run`, or restart the
installed service) to clear it; nothing restarts it automatically on drift.

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
`KeepAlive`, but does not load it. The plist carries `PATH` and the configured
`overseer.session_env`, and is written mode `600`; a launchd agent inherits nothing else,
so anything the daemon needs in its environment has to be there. Configure the credential
its sessions run under **before** installing — see
[Session credentials](#session-credentials) — or the judge, triage, and review sessions
will fail to authenticate even though an interactive `claude` on the same machine works.

If Discord is enabled, first make the already-exported token available to the user's
launchd environment, then load the service using the exact path printed by the install
command:

```sh
launchctl setenv ROBCO_DISCORD_TOKEN "$ROBCO_DISCORD_TOKEN"
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.robco.overseer.plist
```

Replace `ROBCO_DISCORD_TOKEN` in that command if `token_env` uses another name.

`robco install` takes the other side of that trade: its launchd step defaults to writing
*and* loading the service, so an operator recovering a dead daemon reaches a running
service by accepting every prompt. The load is verified — a `launchctl bootstrap` that
exits 0 without producing a loaded service fails the run instead of reporting success —
and a wizard that ends with dispatch enabled while the service is still down closes with
an explicit warning naming the recovery commands. `install-service` stays non-executing
because it is the scripted, copy-the-command path: it is invoked from runbooks and
non-interactive setups where loading the service is a separate, deliberate step (and,
with Discord enabled, must follow the `launchctl setenv` above).

Inspect it at any time with:

```sh
robco overseer status
```

For a foreground daemon, `robco overseer stop` sends `SIGTERM` and waits briefly. For the
installed KeepAlive service, unload it to stop it without an immediate relaunch:

```sh
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.robco.overseer.plist
```

Daemon stdout and stderr go to `~/.robco/overseer/overseer.log` under launchd. Each spawned
session's stderr goes to `session.log` in its own case directory under
`~/.robco/overseer/{judge,triage,review,preflight}/`.

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

### Ledger retention

Nothing but a detach used to remove a ledger entry, so everything that reached `merged`,
`failed`, or `escalated` stayed on the ledger for the life of the installation — a file
rewritten on every save and cloned on every reconcile pass. `terminal_retention_per_repo`
bounds that: at the end of each pass, the settled entries ranked outside the most recent
N of their repository are dropped, and each drop is recorded in `decisions.jsonl` under
source `retention` with the entry's task, repository, and pull request. Repositories are
counted separately, so a busy one cannot push another's history off the ledger, and the
window is a count rather than an age because an entry records when it was dispatched, not
when it settled.

The window is applied last, after reconcile, notifications, triage, merge, and dispatch
have read the board, so it only decides how much settled history the *next* pass
inherits. It never drops a non-terminal entry — live work holds a worktree and a dispatch
slot, so `active_workers()`, `max_workers`, and `per_repo_limit` read exactly the counts
they read before — and it never drops a terminal entry whose worker is still registered:
merged cleanup is re-pushed for as long as the registry row survives, and dropping the
entry first would leak the session and worktree that cleanup was about to remove. A
`failed` or `escalated` worktree left standing for an operator therefore stays visible for
as long as it exists, though it still occupies a place in its repository's window.

Retention is the one thing that erases dispatch history: `max_retries_per_task` counts a
task's recorded entries, so a task whose entries have all aged out is one Overseer no
longer remembers attempting, and a still-ready one may be dispatched afresh. That is the
intended reading of a retention window — a task untouched across the last N settlements
of its repository is being started again, not retried — and `skip_list` remains the
durable way to say never.

The JSONL decision log is the durable audit trail used by `robco overseer status`, the TUI
Overseer info pane, and Discord notifications. The daemon writes observation snapshots
separately so a failed probe becomes a logged skipped observation instead of invented
state.

### The Inbox in the TUI

The `Inbox` category under `OVERSEER` aggregates what is waiting on the operator: every
escalation the daemon recorded, plus every worker sitting on a confirmation prompt. The
category row summarises it as `N/M actionable` — how many of the listed items have a live
tmux session to answer into.

Expanding the category (`l`, `→`, or `Enter` on the category row) turns each item into a
row of its own — one level, with no repeated count row between the category and its items,
because the category row's own `N/M actionable` summary already carries the count. Those
rows are ordinary tree rows: `j` / `k` move onto them, they carry the tree's own selection
marker, and `h` folds them back under the category. There is no second cursor and no key
that only works while a particular preview tab is showing — an item lives in the left
frame, so what the right pane happens to display never decides what acting on it does.

Selecting an item previews *that item*: its kind, target, session, timestamp, and its
reason in full. The sidebar trims the row label to the frame width, so the pane is where
the whole escalation is readable. It deliberately does not re-list the other items — they
are already on screen a few columns to the left.

Four keys act on the selected item:

- `Enter` opens the answer prompt. Submitting sends the text, then `Enter`, to that item's
  tmux session — the same thing an operator would type after attaching.
- `y` approves it, sending `y` + `Enter` to the same session.
- `d` dismisses it.
- `D` clears the whole Inbox, behind a confirmation. It is also bound on the Inbox category
  row itself.

An item whose worker is dead or branch-only has no session to answer into. It is still
listed — the escalation is real and the operator still needs to see it — but it renders as
`display-only`, and both `Enter` and `y` say so rather than appearing to send something.
`Enter` on such a row never falls through to attaching a session.

### Dismissing Inbox items

The Inbox is derived, not stored: it is rebuilt from the decision log, the ledger, and the
registry on every refresh. Dismissing therefore cannot delete a record, and does not try
to — it writes a suppression to `~/.robco/overseer/inbox_dismissals.json` that the
aggregation applies as a filter. `decisions.jsonl` and `ledger.json` are never touched.

A suppression records the item's `(kind, target_id)` identity plus the timestamp the row
carried when it was dismissed, and hides only items at or before that timestamp. A *newer*
escalation for the same target is listed again — otherwise clearing one stale alert would
silently mute that task forever, which is the failure mode the whole design exists to
avoid. Entries whose target no longer appears in any source are pruned on the next write,
so the file does not grow one row per escalation the Overseer has ever raised.

This is what the ledger-sourced rows needed. A ledger entry parked at `phase=escalated`
never ages out on its own, unlike a decision-sourced row, which falls out of the Inbox once
enough newer decisions accumulate. Before dismissal existed, the only way to clear one was
to stop the daemon and hand-edit `ledger.json`.

`robco overseer clear-inbox` is the scriptable equivalent of `D`: it aggregates the same
three sources and suppresses everything they currently produce, so the Inbox can be cleared
with the TUI closed.

The list is re-aggregated on every refresh and sorted newest-first, so a newly arrived
escalation shifts the rows below it. The cursor is re-anchored by the item's own identity
rather than by its position, so an arrival cannot slide the selection onto a different
worker between the operator reading a row and pressing `y` on it.

### Worktree management in the TUI

`g` is the only key for this axis. On a worktree row it cycles
unmanaged → Overseer Auto → Overseer Manual → unmanaged, so one key both enrolls a
worktree and detaches it again. The cycle position is read from ownership and mode
together: a worktree created by hand under `worktree_root` is adopted as unowned but
persisted as `Manual`, and the first `g` overwrites that stale mode when it enrolls.
There is no confirmation prompt — every step is non-destructive and two more presses
undo it.

The tree reports this axis with an accent-coloured `▶` in the cell **immediately left of the
row's title**, right of that row's indentation, so the marker is indented with the tree
hierarchy and belongs to the agent rather than to a fixed row-head column. Only Auto workers
are marked: a Manual worker and an unmanaged worktree both render blank there, and the
difference between them is read from the OVERSEER info pane rather than from the tree.

Adoption derives the mode from the parent it recovers, not from a fixed default. A worker
whose live session still carries `ROBCO_PARENT_AGENT_ID=overseer` is re-adopted as an
Overseer child *and* as `Auto`, so a worker that lost its registry row comes back under
automatic dispatch instead of returning as one the dispatch gate skips as manual. A
hand-made worktree has no such session to recover a parent from and is stored `Manual`
until `g` enrolls it.

A worktree that already belongs to *another agent* is off the cycle and `g` declines it.
`parent_agent_id` records both Overseer ownership and the identity-tree parent, and
nothing persists what the parent was before enrollment, so an Overseer-managed worker is
defined as never also being another agent's child rather than having its parent silently
overwritten.

Manual workers remain owned by Overseer but are skipped for automatic dispatch. Manual
suppresses intervention, not occupancy: a live Manual worker still holds a worktree, a
branch, and a tmux session, so it counts toward `max_workers` and `per_repo_limit`
exactly like an Auto worker, and `robco overseer status` reports the same count the
dispatch gate enforces. Cycling a worker to Manual therefore never frees a dispatch slot.

Manual stops the auto-merge gate too, and the gate says so. A Manual worker's pull request
is never merged by Overseer — that part does not change — but an entry sitting at
`pr_opened` with a pull request open is a merge candidate Overseer *declined*, not one it
never reached, and the two used to look identical from `decisions.jsonl`. The pass records
a `manual` skip under source `auto_merge` carrying the pull request URL. It is recorded
once per pull request rather than once per poll pass: management is a standing state, so a
per-pass entry would bury the log the way the silent skip hid in it. While the state
stands, `robco overseer status` and the OVERSEER ledger detail both report
`merge-eligible, manual: N`. Cycling the worker back to Auto clears the marker, so a later
switch to Manual is recorded again. The skip happens before the gate reads the pull
request, so a Manual worker's pull request is never handed back by merge recovery either.

Detaching does free one, because it ends Overseer ownership entirely. The next daemon
pass sees a worker that is no longer its child and **drops that worker's ledger entry**,
logging the drop to the decision log. The entry is removed rather than marked terminal:
`failed` would report a failure to dropr that never happened, and `merged` would run the
post-merge cleanup that kills the session and removes the worktree — the opposite of a
detach, which leaves the worker and its tmux session running. Use the separate kill
action when the worker should also stop. From there the worktree is exactly a hand-made
one: Overseer neither tracks nor counts it. Re-enrolling it with `g` restores ownership,
and the entry comes back through the same startup adoption pass that picks up every other
Auto child — that pass runs when the daemon starts, so a re-enrolled worker is re-entered
on the ledger at the next daemon start.

The drop is keyed on ownership, not on mode — a detached worker keeps its `Manual` mode,
and an entry whose agent has left the registry altogether is dead, not detached, so it
still travels the session-death path.

On a repo row `g` acts on every worktree under the repo behind a confirmation, keeping
the stand-down bias: any Auto worker present sets every Overseer-managed worker to
Manual and leaves unmanaged worktrees alone, and otherwise every worktree the Overseer
may touch becomes Auto, enrolling the unmanaged ones. The repo-level action never
un-enrolls; detaching stays a per-worktree decision.

Only worktrees under `config.worktree_root` reach the tree at all — one created
elsewhere is never adopted, so no key can bring it under Overseer management.

### Repo-level Overseer opt-out

`g` cycles individual worktrees; it says nothing about whether the Overseer looks at a
repo at all. `G` on a repo row is the separate, coarser toggle: it flips `RepoNode`'s own
`management` field between Auto and Manual, reusing the same `ManagementMode` vocabulary
as every worker's own field rather than inventing a third enum, so a repo and a worker
inside it can be compared directly.

A repo switched to Manual is dropped from `gather_candidates` before its dropr workspace
is even looked up, recording an `overseer_unmanaged` skip per pass — its own reason,
distinct from `workspace_unmatched`, so an idle Overseer stays diagnosable from
`decisions.jsonl` alone. Auto-merge honours the same decision: `worker_is_auto` now
requires the entry's repo to be Auto *and* its worker to be Auto, so a Manual repo's pull
requests take the existing per-worker `manual` skip path in `auto_merge_pass` — recorded
once per pull request, exactly like a Manual worker's. The two gates were already at risk
of disagreeing about a worker inside a repo the operator opted out of by hand; this closes
that gap rather than adding a second way for them to.

Switching a repo to Manual does not touch workers already running under it — no worktree
is removed, no tmux session is killed, and existing ledger entries are left to reach their
own terminal phase (or sit on the same Manual skip a per-worker toggle would produce, if
one is still open). Only future dispatch and auto-merge passes read the new state. Use the
existing kill or detach actions to affect a worker directly.

The repo row carries its own marker unconditionally — unlike a worker row's, it never
blanks out, since it is the state every worker row underneath is compared against. A
worker row's own marker is shown only when it *diverges* from its repo's: a worker left at
Auto under an Auto repo repeats what the repo row already said and renders blank, while a
worker explicitly set to Manual under an Auto repo (or vice versa after `G`) keeps its own
glyph. A Manual repo's name and every worker row under it render dimmed, so the opted-out
state reads at a glance without hunting for the marker cell. `robco overseer status`
reports a `repos: N watched, M opted out: <names>` line so the same state is visible
without opening the TUI.

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
