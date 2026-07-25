# 12 — MCP Tool Surface

`robco mcp` serves a stdio MCP server. Its tools let a controller agent drive RobCo
without a human at the TUI: create workers, watch them, answer their prompts, and land
their work.

This page documents what each tool does, when it refuses, and which ones change
repository state. It deliberately does not restate the JSON schemas — call `tools/list`
for those; every tool declares its `inputSchema`, and the ones with a structured result
declare an `outputSchema` too.

Every tool identifies an agent by its registry id (`agent_id`), which
`robco_agent_list` and `robco_whoami` both return. An id that is not in the registry is
an error, never a silent no-op.

## What mutates what

| Tool | Effect |
|------|--------|
| `robco_whoami`, `robco_agent_list`, `robco_agent_status`, `robco_question_list`, `robco_overseer_policy`, `robco_pr_status` | Read-only. |
| `robco_report`, `robco_answer`, `robco_approve`, `robco_pr_request` | Send input to a session or an inbox. Nothing in the repository changes directly — but the agent that receives the text may change plenty. |
| `robco_agent_create` | Creates a branch, a worktree, and tmux sessions. |
| `robco_merge` | **Destructive.** Merges the pull request, removes the worktree, deletes the branch locally and on the remote, kills the agent's sessions, and drops it from the registry. RobCo cannot undo any of it. |

## Identity and coordination

- **`robco_whoami`** returns the calling agent's `ROBCO_AGENT_ID` and
  `ROBCO_PARENT_AGENT_ID`, plus its title and repository when the id is still in the
  registry. See [Agent reporting](10-agent-reporting.md).
- **`robco_report`** sends a single-line message to a controller. Its exit conditions and
  the equivalent CLI are documented in [Agent reporting](10-agent-reporting.md).
- **`robco_agent_list`** and **`robco_agent_status`** report live status, including
  `tracked_command` and `subagents_active`. Probe failures degrade to `null` / `0` rather
  than failing the request.
- **`robco_question_list`** lists agents currently waiting on a confirmation prompt;
  **`robco_answer`** sends text plus Enter to a session, and **`robco_approve`** sends
  `y` plus Enter.
- **`robco_agent_create`** creates a worker in a registered repository.
- **`robco_overseer_policy`** reads the Overseer daemon's local policy and health. See
  [Overseer Agent](11-overseer-agent.md).

## PR tools

### `robco_pr_status`

Reports what the agent's branch has on GitHub, as one of four states:

| `pr_state` | Meaning |
|------------|---------|
| `open` | At least one pull request is open, so it can still be merged. |
| `merged` | None are open and one landed; only cleanup is left. |
| `closed_unmerged` | Every pull request was closed without merging, so the branch still holds work that is nowhere else. |
| `absent` | The branch has never had a pull request. |

The four states are kept distinct because a controller has to act differently on each,
and an open-only query would flatten the last three into "no PR" — which would read as
"open one" for a branch whose work was already merged or deliberately abandoned.

A branch can carry several pull requests at once; the states are ranked rather than read
off the first entry. An open pull request outranks everything, and a merge outranks an
abandoned attempt.

It fails rather than guessing when `gh pr list` cannot be read: reporting `absent` for an
unreadable response would tell the caller to open a pull request it may already have.

### `robco_pr_request`

Asks an agent to open its pull request.

**RobCo does not run `gh pr create`.** Authoring the pull request is the worker's job;
this tool only delivers a prompt into the worker's tmux session, exactly as the TUI's
`p` key does. A successful call therefore means *the prompt was delivered*, not that a
pull request exists. Poll `robco_pr_status` to find out whether the agent acted on it.

`prompt` is optional; omitting it sends the configured `pr_prompt` (see
[Config Reference](09-config-reference.md)), which is the text the TUI pre-fills its
dialog with. A blank explicit prompt is refused rather than sent.

It refuses when:

- the agent's tmux session is not running — there is nothing to deliver the prompt to;
- the branch already has an open pull request — asking for a second one produces
  duplicate work, not a second PR.

## `robco_merge`

Runs the same sequence as the TUI's merge key, in the same order, so a worker landed over
MCP ends up in the state a worker landed by hand would:

1. `gh pr merge` with the configured `merge_strategy` (skipped in `clean_only` mode);
2. fast-forward the repository's primary worktree;
3. remove the agent's worktree, then delete the branch locally and on the remote;
4. kill the agent's tmux and shell sessions;
5. tell the Overseer daemon a merge completed, so it reconciles on a pass that starts now
   rather than up to a poll interval later;
6. drop the agent from the registry.

A step that fails aborts the sequence — the remaining steps do not run. That is
deliberate: the caller is watching, and a half-finished cleanup reported as success is
worse than a stop with a named cause. (The Overseer daemon's own cleanup runs the same
steps under a *continue* policy instead, because nobody is watching it and a base branch
that refuses to fast-forward must not strand a worktree forever.)

### Gates

`robco_merge` is gated twice, because nothing it does can be undone by RobCo:

- **`confirm` must be `true`.** It has no default. The check runs before the registry is
  even read, so a call that omitted it cannot reach anything that touches the repository.
  This is the MCP counterpart of the TUI's confirmation dialog.
- **The pull request must be in the state the mode expects.** `merge_then_clean` (the
  default) requires an `open` pull request; `clean_only` requires a `merged` one. A
  mismatch is refused with both states named. This is what stops `clean_only` from
  deleting a branch whose work was never merged, and stops `merge_then_clean` from being
  aimed at a pull request that already landed.

The Overseer's `auto_merge` policy is **not** a gate here. It governs whether the daemon
merges on its own initiative; `robco_merge` is an explicit request from a caller that
already said `confirm: true`. Read it with `robco_overseer_policy` if a controller wants
to honour the operator's intent for unattended merges as well.

### Results

The tool returns structured data rather than prose, in both directions:

- `steps` lists the steps that started, in order (`merging PR`, `pulling main`,
  `cleaning up`).
- `ok` is `false` when the sequence failed. **A failed merge is a normal result, not a
  tool error** — the call ran, the merge did not. `failed_step` then names the step it
  stopped on and `error` carries the text, so a controller can report the failure without
  parsing it.

Argument and precondition problems (`confirm` missing, unknown `agent_id`, wrong pull
request state) are still tool errors: nothing was attempted.

### Concurrency

The sequence races on the repository's base branch and working tree, so only one may run
per repository at a time. The TUI serialises its own merges in memory, which says nothing
about a separate `robco mcp` process, so the shared sequence takes a per-repository
advisory lock under `~/.robco/merge-locks/` that both surfaces respect. A merge issued
while another RobCo process is merging in the same repository is refused with that reason
rather than queued — both callers would rather be told than blocked for however long the
other one takes.

Because the lock is an advisory `flock`, the kernel releases it when its holder exits.
There is no stale lock to reap: a RobCo that crashed mid-merge leaves the next one free
to run.

Merges in *different* repositories share no state and run concurrently.

The Overseer daemon's post-merge cleanup takes the same lock, so it cannot overlap with a
merge either. The daemon does not wait for it: a pass covers every repository it watches,
and one blocked on a merge would stall the rest. A cleanup that finds the lock held is
logged as deferred and re-emitted on the next reconcile pass.
