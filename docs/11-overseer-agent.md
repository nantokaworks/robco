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
   observation is appended to `~/.robco/overseer/observations.jsonl`. Separately, each
   watched repository's open pull requests are listed and diffed against the ledger, so
   ones Overseer never dispatched (a Dependabot bump, a human's own branch) still surface
   in the TUI's repository INFO pane instead of staying invisible. This listing backs off
   per repository — five minutes between re-lists — rather than running every poll, and
   is cached in `~/.robco/overseer/other_prs.json`, kept apart from the ledger since it
   records nothing Overseer dispatched.
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
rest of that repository is held with `repo_merge_settling` until the post-merge
`git pull --ff-only` lands, because the merge advanced their base. The hold covers the
merge itself and nothing else — the pull request now at the head of the queue still reads
GitHub and updates its branch in the same pass, which is what stops each extra pull
request costing a whole poll interval. Repositories remain independent of each other.
Workers are never instructed to merge their own pull requests.

### Execution plane

Each dispatched task gets one RobCo worker: one git worktree, one branch, and one tmux
session registered with parent id `overseer`. The worker receives an assignment prompt
that hands it the claim Overseer already holds on its assigned dropr task, and requires
it to verify that claim rather than take one, report lifecycle changes, commit and push
its branch, and open (but not merge) a pull request.

All three names are built from one slug that leads with the task's number and source —
dropr task `#295` becomes `295-dropr-<title>`, capped at 32 characters on a hyphen
boundary. The number leads because it is what the operator actually scans a column of
names for; the source segment right after it keeps the origin of the number readable and
leaves the numbering space open for a second task source later. Existing workers keep the
names they were created with; the shape applies to newly dispatched ones.

An Overseer-dispatched agent's tree row leads with the same number (`#295 <title>`), read
from the dropr display id captured once at spawn time rather than re-derived from the
name above. A manually-created or adopted agent carries no such number and its row is
unchanged.

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
spawned, because there is no model to run. `robco overseer status --debug` reports the
judge and review counts separately, and names the two states apart — `findings every 20m,
no reviewer model` against `every 20m via <profile>` — so a quiet board can be read as
"nothing was found" rather than "nothing looked".

Task text, exception reasons, tmux capture, and other external values are each placed in
explicit `EXTERNAL_DATA` delimiters. Closing delimiter text inside a value is escaped. The
briefing tells the LLM to treat every such field as data, not as instructions. The Discord
ops agent is the one exception: an allow-listed operator's message is the session's actual
instruction, so the briefing hands it to the model as an instruction, not as fenced data —
only the ledger status, decision log, case context, tmux capture, and retained conversation
history (dropr:363, see [Retained channel agents](#retained-channel-agents) below) around it
stay fenced. The operator's message is still text from Discord, so both the opening and
closing `EXTERNAL_DATA` fence syntax are neutralized inside it before it is embedded, so it
cannot forge a data block that shadows the real ones below it. Discord-generated impactful
actions still pass through the same human confirmation gate as typed commands.

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
    "allow_unverifiable_protection": false,
    "autonomy_level": "conservative",
    "max_branch_updates": 3,
    "max_merge_judge_primes": 3,
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
    "release_pipeline_enabled": false,
    "discord": {
      "enabled": false,
      "token_env": "ROBCO_DISCORD_TOKEN",
      "channel_id": null,
      "allowed_user_ids": [],
      "notify_level": "summary",
      "notify_localize": true,
      "chat_category_ids": [],
      "chat_concurrency_cap": 3,
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
| `allow_unverifiable_protection` | boolean | `false` | Lets auto-merge proceed on a repository whose GitHub plan cannot answer the protection probe at all (`unprotected:plan_unsupported`), instead of holding it forever. Security-relevant: a plan-limited `403` is indistinguishable from a repository whose owner never configured protection, so enabling this accepts merges onto a base branch Overseer could never confirm is protected. It has no effect on a repository whose plan can actually answer the probe — those are still held on their real facts. |
| `autonomy_level` | `"approval_only"`, `"conservative"`, or `"full_auto"` | `"conservative"` | How much of the merge envelope the daemon may clear without an operator. `approval_only` escalates every merge; `conservative` auto-merges only a docs-or-tests change under 5 files and 200 lines that trips no risk; `full_auto` escalates just the hard stops — destructive changes, security-sensitive changes, repeated failures, an exhausted LLM budget, and external side effects. Set it with `robco overseer autonomy <level>`. |
| `merge_strategy` | — | — | Retired. The strategy is the top-level [`merge_strategy`](09-config-reference.md#merge_strategy), which the TUI reads too, so the two merge paths cannot disagree. A config still carrying this key is migrated on load and the key is dropped on the next write. |
| `max_branch_updates` | non-negative integer | `3` | Times the auto-merge gate may update one pull request's branch onto its base before escalating that entry. Each attempt is charged before it runs, so an update that fails still spends budget. `0` never updates a branch and escalates the first time one falls behind. |
| `max_merge_judge_primes` | non-negative integer | `3` | Merge judgements the gate may start for one pull request *early* — while it is still waiting on its checks — rather than after the gate clears. A judgement is keyed on the change, and every push mints a new one, so without this a worker pushing many CI fixes could spend the whole `daily_llm_budget` on one pull request. Charged before the judgement is queued. `0` turns early judgements off, leaving every merge judgement to run after the gate clears. |
| `merge_recovery_enabled` | boolean | `false` | Hands a merge failure the owning worker could fix back to that worker's live session instead of parking the pull request. Default-off, so a daemon that has never heard of merge recovery behaves exactly as it did before it existed. Switched off, each failure it would have acted on is still recorded once per revision as `merge_recovery_disabled:<reason>` and counted into `merge-recovery: off (N dropped)`. |
| `max_merge_recoveries` | non-negative integer | `2` | Handbacks one pull request may be charged before it escalates to an operator. Each attempt is charged before it runs, so a handback that never reaches its worker still spends budget. `0` never hands anything back and escalates the first recoverable failure. |
| `max_merge_holds` | non-negative integer | `30` | Auto-merge passes one pull request may be held under the same reason at the same head before the entry escalates with `merge_hold_cap_reached:<reason>`. Without it every non-merge exit re-records its reason once per poll for as long as the condition lasts. At the default `poll_interval_secs` the default is thirty minutes — past the 5-15 minutes a healthy check run takes, and well inside an hour. Exits with their own budget (`behind_*`, the settle barrier) are not charged twice. `0` escalates on the first held pass. |
| `max_merge_hold_rechecks` | non-negative integer | `10` | Further looks through the gate an entry escalated by `max_merge_holds` is given, so a condition an operator fixes afterwards is noticed instead of leaving the pull request parked for good. Only a pass that re-read the gate and found it still holding spends one; a pass waiting on a judgment spends nothing. The pass that spends the last look records `merge_hold_recheck_exhausted:<reason>`. `0` leaves an escalated entry where it is, which is how Overseer behaved before this budget existed. |
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
| `release_pipeline_enabled` | boolean | `false` | Runs `scripts/release.sh` unattended, from this project's own checkout, after a merge closes a `[release]`-scoped task in this project's own repository. A distinct privilege class from every other flag above: on success it publishes a public GitHub release with whatever credentials the daemon holds, and `scripts/release.sh` is itself part of this repository, so a future change to it runs with this same privilege on the next qualifying merge. Default-off; an operator opts in deliberately. See [`overseer::release_pipeline`](../src/overseer/release_pipeline.rs). |
| `discord` | object | see below | Discord gateway, command, and notification settings. |

### Discord fields

| Key | Type | Default | Implemented behavior |
|-----|------|---------|----------------------|
| `enabled` | boolean | `false` | Starts the supervised Discord gateway thread. |
| `token_env` | string | `"ROBCO_DISCORD_TOKEN"` | Name of the environment variable containing the bot token. The value is never read from config. |
| `channel_id` | string or `null` | `null` | Allowed parent channel id and notification destination. It must parse as a non-zero integer. |
| `allowed_user_ids` | array of strings | `[]` | Exact Discord user-id allowlist. The bot refuses to start when it is empty. |
| `notify_level` | `"off"`, `"errors"`, `"summary"`, or `"all"` | `"summary"` | Notification verbosity baseline, and the sole gate for which events post. See [Notification level](#notification-level) below. |
| `notify_localize` | boolean | `true` | Runs notification titles and descriptions through an LLM pass in the top-level [`language`](09-config-reference.md#language) before posting. No effect while `language` is unset or blank — the pass is always skipped then, regardless of this key. A localization failure (timeout, launch failure, malformed result) still delivers the English text; nothing is ever dropped. |
| `chat_category_ids` | array of strings | `[]` | Discord category IDs whose text channels get a conversational reply to plain chat, exactly like `channel_id` already does — same `allowed_user_ids` boundary, no command prefix needed. Empty leaves behavior byte-identical to before this key existed: no channel lookup is ever attempted. See [Category-scoped chat](#category-scoped-chat) below. |
| `chat_concurrency_cap` | non-negative integer | `3` | Concurrent ops-agent sessions across `channel_id` and every `chat_category_ids` channel combined, each a spawned OS thread running an agent CLI. A channel beyond the cap gets the same "handling another request" reply a single-channel agent already returned, rather than a dropped message. |
| `action_limit_per_hour` | non-negative integer | `30` | Maximum mutating Discord actions in a rolling hour. Attempts count when execution begins. |
| `confirmation_ttl_secs` | non-negative integer | `120` | Lifetime of an impactful command's confirmation nonce. |

### Notification level

`notify_level` replaces the seven per-event booleans above with a single verbosity dial. Every
Discord event has an intrinsic tier, and a level admits an event when the event's tier is at or
below the level:

| Level | Fires |
|-------|-------|
| `"off"` | Nothing. |
| `"errors"` | `task_failed`, `task_escalated`, `worker_blocked`, a circuit-open, and a generic escalation. |
| `"summary"` (default) | Milestones + problems: everything in `errors`, plus a successful task finish (`merged`). |
| `"all"` | Everything in `summary`, plus the step-by-step events: `task_started`, `pr_opened`, and `queue_drained` (see [Queue-drained notification](#queue-drained-notification) below). |

**`"summary"` means milestones only.** A milestone is something the operator acts on or wants
to know happened — work landing (`merged`) or breaking (everything in `errors`). Step-by-step
narration (a worker starting, a PR opening, the queue going idle) is `"all"`-tier: with several
repositories dispatching many tasks a day, those events alone turn `"summary"` into a steady
stream.

**Merge rollup.** At any level that admits `merged`, several merges landing within a short
window (5 minutes, fixed) post as one rolled-up message — `3 pull requests were merged.` with a
`PRs` field listing each repo and PR link — instead of one message each. A lone merge waits out
the window before posting. Errors and escalations are never delayed: an error queued behind
held merges flushes them immediately, and the error itself posts right after. The hold keeps no
in-memory state — held entries simply stay unacknowledged on the delivery cursor, so a daemon
restart mid-window loses nothing.

**This changes the default.** Before `notify_level` existed, every event fired unconditionally
(equivalent to `"all"`). The wizard and a fresh `DiscordConfig::default()` both now default to
`"summary"`. Set `notify_level` to `"all"` to restore the old behavior in full.

**Legacy per-event overrides.** An earlier revision of this feature kept seven per-event
`notify_*` booleans (`notify_escalation`, `notify_pr_opened`, `notify_merged`, `notify_circuit`,
`notify_worker_blocked`, `notify_task_started`, `notify_task_finished`) alongside `notify_level`,
each an explicit override that took precedence over the level when set. Those keys have since been
removed: `notify_level` is now the only gate. A config file still carrying one of the seven still
loads — the key is silently ignored — but the daemon logs a one-time notice at start-up naming
which keys were found, so an operator who relied on one of them (`notify_pr_opened = true` in
particular, since `pr_opened` is `all`-tier and `summary` is the default) learns that raising
`notify_level` is now the way to get that event back.

#### Queue-drained notification

Fires once when the Overseer's ready-candidate gather succeeds and finds nothing left to do: no
ready candidates from any repository, and no ledger entry still in a non-terminal phase. It is
edge-triggered — logged on the pass that first observes the drained state, not on every poll
after — and the edge is persisted (`~/.robco/overseer/queue_drained.json`) so a daemon restart
neither re-announces a drain that already fired nor announces one that never happened (a daemon
started against an already-empty board stays silent).

The condition deliberately excludes a failed gather. `dropr_overlay_unavailable` (the workspace
overlay being unreachable) also produces zero candidates, but that is an outage, not an empty
board — treating it as a drain would misreport a transient failure as "all done", so a failed
gather is skipped entirely rather than evaluated.

Re-arming is state-transition based, not time-based: the next `queue_drained` only fires after a
later pass observes the board **not** drained (new work appeared) and then drained again. There is
no separate debounce timer — a board oscillating between one ready task and zero would need an
actual dispatch-and-settle cycle to flip the persisted state, and a settle cycle already spans many
poll intervals on its own.

### Category-scoped chat

`chat_category_ids` widens conversational chat (plain messages, no `!` prefix) from the single `channel_id` to every text channel under a listed Discord category. The `allowed_user_ids` boundary is unchanged: robco can kill workers and drive merges, so widening *who* it talks to is a separate decision from widening *where* — a category-member channel simply gets the same treatment `channel_id` already gets, nothing more. `!`-prefixed commands work in a category channel too, for the same reason they already work in a triage thread: the confirmation and rate-limit machinery is per-user, not per-channel, so there is no separate boundary to relax.

- **Resolving membership.** A Discord `MESSAGE_CREATE` payload carries no `parent_id`, so membership is resolved with an HTTP `GET /channels/:id` on first sight of a channel, cached in-process (5 minute TTL, oldest-evicted past a bounded size) so a busy category does not re-fetch on every message. No gateway intent is added for this — the existing message-content intent is enough, and the same `channel(id).model()` fetch the triage-thread reconciler already makes. An empty `chat_category_ids` skips the cache and the fetch entirely; a channel that is already the parent channel or a known triage thread skips it too, since both already qualify on their own.
- **Threads are out of scope.** A thread under a category channel is two hops from the category (thread → channel → category); this feature resolves only the first hop, so a thread inside a category channel does not itself inherit chat access. A triage thread still works exactly as before, through its own `is_thread` mechanism, unrelated to categories.
- **Concurrency.** Conversational sessions used to share one global slot; scoping chat to a whole category means people in two different channels can now talk to the agent at the same time, so the slot became a per-channel map bounded by `chat_concurrency_cap`. Each session is a spawned OS thread running an agent CLI, so the cap is a real resource bound, not just a Discord-noise knob. A channel beyond the cap gets the existing "handling another request" reply instead of a dropped message.

### Retained channel agents

Every channel that has ever talked to the ops agent gets a retained record under
`~/.robco/overseer/discord-ops/channels.json` (dropr:363): first contact, last activity, a
turn count, the outcome of the most recent turn, and a bounded rolling transcript (the six
most recent exchanges). Retention here means identity plus conversation continuity, not a
resident process — each turn is still the same spawned, `EphemeralSession`-backed OS thread
described above, bounded by `chat_concurrency_cap` exactly as before. What survives between
turns is the record on disk, not a running process: the next turn's briefing folds the
retained transcript in as a `CONVERSATION_HISTORY` block, fenced and escaped the same way
every other externally-sourced field is, so the agent can refer to what it just said or did
without a person having to restate context every message. A `Running` status left behind by
a daemon that died mid-turn is cleared back to `Idle` the next time the state loads, since
nothing about that turn survives the restart.

The Overseer TUI's INFO pane lists these under its own `Discord` category, one row per
channel — status, turn count, and how long since it last spoke — sorted newest-activity
first. The gateway that writes `channels.json` runs inside the daemon process; the TUI is a
separate process with no access to the daemon's memory, so the pane can only ever show what
made it to disk.

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
robco overseer status --debug            # check the `session auth:` line
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

One name is exempt from that consequence rather than from the blocklist:
`overseer.discord.token_env`. The channel excludes it outright, from both the config map and
the env file, so a token that can post to the ops channel never reaches a worker's tmux
environment or an ephemeral session — even though the wizard writes it into the same file
every other credential in this channel lives in. The Discord gateway resolves that name on
its own (see
[Discord application](#discord-application)); this channel is not involved.

### Health

With `session_preflight` on (the default) the daemon spawns one probe session at start-up
and records the verdict in `~/.robco/overseer/session_health.json`. Any live session that is
refused on credentials overwrites the same record. `robco overseer status --debug` prints
it:

```
session auth: ok (CLAUDE_CODE_OAUTH_TOKEN via session env file, checked 3m ago)
session auth: failed (no credential configured, checked 0m ago) — Failed to authenticate: OAuth session expired and could not be refreshed
```

A failed probe also lands in the default (non-`--debug`) `stuck:` reasons — an operator
never has to know to pass `--debug` to learn a worker cannot authenticate.

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

A repository whose GitHub plan does not include branch protection at all answers a probe
with `403` — a private repository without GitHub Pro, for example — and neither probe
ever supplies a usable fact. That is a permanent fact about the plan, not a transient
probe failure, so it is held as its own reason, `unprotected:plan_unsupported`, and
cached for an hour rather than five minutes: retrying every poll would keep spending two
`gh api` calls a minute to reconfirm something that only changes when an operator
upgrades the plan. Set [`allow_unverifiable_protection`](#overseer-fields) to let
auto-merge proceed on such a repository anyway; it does not loosen verification on any
repository whose plan can actually answer the probe.

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

Only one pull request per repository updates its branch in a pass. Whichever one the pass
reaches first while GitHub calls it mergeable or behind is that repository's head of
queue; the rest are held under `behind_not_next`, because updating them now would only be
undone the moment the head merges. The slot is given back the moment its holder leaves the
queue — it merged, or it escalated — so the pull request behind it starts its own update
in the same pass rather than a poll interval later. Draining a queue of ready pull
requests therefore costs one branch update each, not one pass each.

The merge judgement runs alongside that wait rather than after it. A pull request the gate
holds under `checks_waiting` or `behind_branch_updated` — or one whose repository is still
settling — is on its way to a merge and the change the judge would read is already final,
so the judgement is started then and the verdict is usually in hand by the time the checks
report. Three bounds keep that from spending model time on changes that never merge: one
pull request per repository per pass, `max_merge_judge_primes` per entry, and the autonomy
envelope, which refuses a change the judge would never be asked about anyway. The
judgement is fingerprinted by the change itself and not by the head commit, so a branch
update does not buy the same verdict twice.

An early judgement is only ever *started* here; the verdict is read where it always was,
by the gate that is ready to act on it.

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

An entry escalated that way is not abandoned. The condition it stopped on — protection,
checks, merge state — is one an operator can fix, and nothing else would ever bring the
entry back: a pre-judge hold never reaches the judge, so the judge-verdict re-entry path
has nothing to offer it. So such an entry is given `max_merge_hold_rechecks` further looks
through the gate. A look is spent only by a pass that re-read the gate and found it still
holding; a pass that clears the gate and waits on a judgment spends nothing, because a
judgment arrives on the judge queue's own schedule — one session at a time — and can
outlast the whole budget, which would strand the entry exactly the way the rechecks exist
to prevent. Once a verdict lands the judge becomes the authority reconsidering the entry
and the remaining looks are retired. The pass that spends the last look records
`merge_hold_recheck_exhausted:<reason>` once, so the log distinguishes an entry still being
re-checked from one nothing will look at again.

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

This worker/operator split is `overseer::remedy::classify`, which the module also uses as
its own fallback: any reason it calls worker-fixable, `overseer::remedy::resolve` treats as
`Answer` even without a dedicated table entry, so the two never drift apart. `remedy` is the
broader authority the Inbox reads from — see [The Inbox in the TUI](#the-inbox-in-the-tui) —
turning a reason into a short move (`ANSWER`, `MERGE`, `RESET`, `RETRY`, `REVIEW`, `WATCH`)
plus a sentence of guidance, rather than only the narrower recoverable/operator question this
section answers.

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
`merge_recovery_disabled:<reason>`, and `robco overseer status --debug` reports the
running total beside the switch as `merge-recovery: off (N dropped)`. The entry keeps its phase and no
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
pass. `robco overseer status --debug` and the TUI OVERSEER frame both report the switch and
its cap.

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

An allow-listed operator can also ask the ops agent to file a dropr task
(`dropr_task_create`, taking a robco-registered repo name, a title, and an optional
description). It resolves the repo the same way `robco spawn --repo` does, maps it onto
the dropr workspace RobCo's registry already associates with that repository's remote,
and creates the task there. Like every other impactful action it is queued behind the
confirmation prompt, counts against the mutating-action rate limit, and is audited on
both success and refusal — there is no separate bypass for it.

## Setup runbook

For first-time setup, run the interactive wizard from a terminal:

```sh
robco install
```

It probes `git`, `tmux`, `gh`, and `dropr`; offers MCP registration; walks through the
Overseer worker, triage, capacity, Discord, and macOS launchd settings; then writes
`~/.robco/config.json` once at the end. Existing values are prompt defaults, so a
second run that accepts every default leaves the configuration unchanged. Discord bot
tokens are never stored in `config.json`: the wizard records only `token_env` there. A
token typed at the following prompt — terminal echo disabled, blank to leave the
existing value alone — is written straight to the session env file instead; see
[Session credentials](#session-credentials). On macOS, the wizard can additionally
copy an already-exported token's current value into launchd, with explicit
confirmation.

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
`robco overseer status --debug` reports it as `version=` beside `pid` and `heartbeat`. When
that version differs from the `robco` binary answering the command — the exact
"installed but not restarted" state — both the status command and the TUI Health frame
warn and name the two builds; the OVERSEER header carries it as a `stale build` warning
row, and the plain (non-`--debug`) `robco overseer status` lists it under `stuck:`. A
heartbeat written before the daemon recorded its build reads as `unknown` and warns the
same way, because only a release older than this one leaves the field out. Restart the
daemon (`robco overseer stop` then `robco overseer run`, or restart the installed service)
to clear it; nothing restarts it automatically on drift.

### Discord application

1. In the Discord Developer Portal, create an application and add a bot.
2. Enable the privileged **MESSAGE CONTENT INTENT** for the bot. Overseer requests
   `GUILD_MESSAGES` and `MESSAGE_CONTENT` gateway intents.
3. Install the bot in the server with only View Channels, Send Messages, Send Messages
   in Threads, Create Public Threads, Manage Threads, Embed Links, and Read Message
   History.
4. Enable Developer Mode in Discord, then copy the operations channel id and each
   operator's user id into `channel_id` and `allowed_user_ids`.
5. Set `token_env` to the environment variable name the bot token is known by, then
   supply the token value itself. `robco install` prompts for both: the wizard's
   Discord step asks for `token_env` and then, with terminal echo disabled while you
   type, the token value. A blank answer to the token prompt leaves whatever is
   already resolvable untouched. The typed value is written to the session env file
   (`overseer.session_env_file`, or `~/.robco/env`) at mode `600` — never into
   `config.json` — so it reaches the gateway the same way a `claude setup-token`
   credential reaches a judge or triage session; see
   [Session credentials](#session-credentials). Exporting the variable yourself
   before running the wizard, or writing it into the env file by hand, both still
   work exactly as before.

The gateway reads the token from its own process environment first, falling back to
the env file when the name is absent there; it is never stored in `config.json`.

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

If Discord is enabled and the token lives in the session env file — because the wizard
wrote it there, or because it was added by hand — no further step is needed: the daemon
reads that file directly, regardless of how it was launched. Load the service using the
exact path printed by the install command:

```sh
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.robco.overseer.plist
```

An operator who instead exports the token into their own shell can still copy it into
launchd's environment, which continues to work exactly as before and takes precedence
over the env file:

```sh
launchctl setenv ROBCO_DISCORD_TOKEN "$ROBCO_DISCORD_TOKEN"
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.robco.overseer.plist
```

Replace `ROBCO_DISCORD_TOKEN` in that command if `token_env` uses another name. Unlike
the env file, `launchctl setenv` does not survive logout or reboot, so it needs
re-running each session unless the env file also carries the token as a fallback.

`robco install` takes the other side of that trade: its launchd step defaults to writing
*and* loading the service, so an operator recovering a dead daemon reaches a running
service by accepting every prompt. The load is verified — a `launchctl bootstrap` that
exits 0 without producing a loaded service fails the run instead of reporting success —
and a wizard that ends with dispatch enabled while the service is still down closes with
an explicit warning naming the recovery commands. `install-service` stays non-executing
because it is the scripted, copy-the-command path: it is invoked from runbooks and
non-interactive setups where loading the service is a separate, deliberate step.

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
escalation the daemon recorded, plus every worker sitting on a confirmation prompt. Each
item's reason is resolved by `overseer::remedy` into a `Move` — `ANSWER`, `MERGE`, `RESET`,
`RETRY`, `REVIEW`, or `WATCH` — and the category row summarises it as `N/M actionable`: how
many of the listed items resolve to something other than `WATCH`. This is independent of
whether the item has a live tmux session — merging a pull request by hand, resetting a
tripped circuit, or reviewing a parked ledger entry needs no session at all, so those rows
count even when their worker is gone. Only `WATCH` — nothing has failed and nothing is
waiting on a human, e.g. checks still running — is excluded.

Expanding the category (`l`, `→`, or `Enter` on the category row) turns each item into a
row of its own — one level, with no repeated count row between the category and its items,
because the category row's own `N/M actionable` summary already carries the count. Those
rows are ordinary tree rows: `j` / `k` move onto them, they carry the tree's own selection
marker, and `h` folds them back under the category. There is no second cursor and no key
that only works while a particular preview tab is showing — an item lives in the left
frame, so what the right pane happens to display never decides what acting on it does.

A row reads `[ESC] REVIEW #296`: the kind code (`ESC` or `?`, the dismissal identity) stays,
and the resolved move's tag replaces the raw reason that used to fill the row — the raw
reason rarely fit the sidebar anyway, and a bare code or a judge's full sentence said
nothing about what to *do*. A `WATCH` tag renders muted; every other tag renders like the
rest of the row.

Selecting an item previews *that item*: its kind, the same resolved move, target, session,
timestamp, a `what this means` / `next step` pair of sentences, and its reason in full. The
sidebar trims the row to the frame width, so the pane is where the whole escalation — and
the guidance about it — is readable. It deliberately does not re-list the other items — they
are already on screen a few columns to the left.

Four keys act on the selected item:

- `Enter` opens the answer prompt. Submitting sends the text, then `Enter`, to that item's
  tmux session — the same thing an operator would type after attaching.
- `y` approves it, sending `y` + `Enter` to the same session.
- `d` dismisses it.
- `D` clears the whole Inbox, behind a confirmation. It is also bound on the Inbox category
  row itself.

An item whose worker is dead or branch-only has no session to answer into, or was never
one to answer in the first place (a merge, a reset, a review). It is still listed — the
escalation is real and the operator still needs to see it — but the preview's `session`
field reads `display-only`, and both `Enter` and `y` say so rather than appearing to send
something. `Enter` on such a row never falls through to attaching a session. An item whose
own move needed an answer but has no session resolves to `REVIEW` instead — `remedy::resolve`
downgrades it, since an instruction with no session to send it to is the operator's problem,
not the worker's.

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

The tree reports this axis with an accent-coloured, single-column marker in the cell
**immediately left of the row's title**, right of that row's indentation, so the marker is
indented with the tree hierarchy and belongs to the agent rather than to a fixed row-head
column. By default the marker is round (`●` Auto, `○` Manual); `project_icon = "nerdfont"`
swaps in a bolt/hand pictograph pair instead (see
[06-ui.md#overseer-management-marker](06-ui.md#overseer-management-marker)). Auto is always
drawn, even on an agent whose repo is also Auto. Manual only renders blank when its repo is
also Manual — the repo row already says as much — and an unmanaged worktree is always blank;
in that one remaining ambiguous case the difference is read from the OVERSEER info pane
rather than from the tree.

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
stands, `robco overseer status --debug` and the OVERSEER ledger detail both report
`merge-eligible, manual: N`; the plain `robco overseer status` lists the same pull request
by name under `waiting on you:`, since it needs the same decision an escalation does — a
human choosing to merge it by hand. Cycling the worker back to Auto clears the marker, so a
later switch to Manual is recorded again. The skip happens before the gate reads the pull
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
state reads at a glance without hunting for the marker cell. `robco overseer status
--debug` reports a `repos: N watched, M opted out: <names>` line so the same state is
visible without opening the TUI.

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
