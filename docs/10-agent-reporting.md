# Agent reporting

RobCo-managed agent sessions expose their registry identity through environment
variables:

- `ROBCO_AGENT_ID` identifies the current agent.
- `ROBCO_PARENT_AGENT_ID` identifies its controller when one is configured.

These values are inherited by programs and hooks started inside the agent's tmux
session. The `robco_whoami` MCP tool returns both ids and, when the current agent is
still in the registry, its title and repository.

## Reading live activity

`robco_agent_list` includes two activity fields on every agent in its repo-grouped
response. `robco_agent_status` returns the same fields for one agent:

| Field | Type | Meaning |
|-------|------|---------|
| `tracked_command` | string or `null` | Best-effort name of a live child command, such as `cargo`; `null` when none can be identified. |
| `subagents_active` | non-negative integer | Claude Code subagents whose activity file is within the running window. Done or stale subagents are not counted. |

Both tools refresh missing activity best-effort from the live tmux process tree and
Claude Code session files. Lifecycle guards suppress subagent counts for dead,
branch-only, or missing-worktree agents. Probe or parse failures are represented as
`null` / `0` rather than failing the MCP request.

## Sending a report

The `robco_report` MCP tool sends a labeled, single-line message to a controller. Its
`message` argument is required; `target_agent_id` is optional and takes precedence
over `ROBCO_PARENT_AGENT_ID`. Control characters in the message are collapsed so
they cannot inject extra input into the target session.

Shell hooks can use the equivalent CLI command:

```sh
robco report --message "turn finished" \
  --target "optional-controller-agent-id"
```

Omit `--target` to use `ROBCO_PARENT_AGENT_ID`. Successful delivery produces no
output. Both interfaces reject self-reports, missing targets, busy targets awaiting
confirmation, dead or missing sessions, and unknown agent ids. Delivery uses tmux
literal input followed by Enter and requires tmux 3.2 or newer.

| Exit code | Meaning |
|-----------|---------|
| `0` | Delivered. |
| `2` | Target is busy awaiting confirmation; retry later. |
| `3` | Invalid CLI arguments, invalid parameters, or no configured target. |
| `4` | Target is unavailable, unknown, or delivery failed. |

## Claude Code Stop hook

Add this entry to Claude Code's `settings.json` to report the latest commit when a
child finishes a turn:

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "robco report --message \"turn finished: $(git log --oneline -1 2>/dev/null || echo done)\" || true"
          }
        ]
      }
    ]
  }
}
```

Claude Code Stop and Notification hooks can fire in any session with these identity
environment variables set. In a RobCo-managed session, the inherited values route
the report to the controller. Outside a RobCo-managed session, no parent target is
configured, so the command exits with code 3 and sends nothing. Append `|| true`, as
in the example, when the hook runner should treat that non-zero no-target result as
a silent no-op.
