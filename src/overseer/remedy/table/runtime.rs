//! Reasons the dispatch, triage, and review passes record — everything that
//! is not the auto-merge gate itself.

use crate::overseer::remedy::{Move, Remedy};

pub(super) const EXACT: &[(&str, Remedy)] = &[
    (
        // `dispatch::runtime::open_circuit` — dispatch stops itself after
        // `failure_circuit_threshold` consecutive spawn failures.
        "dispatch disabled pending operator reset",
        Remedy {
            step: Move::Reset,
            means: "the dispatch circuit tripped after repeated consecutive failures, \
                    so dispatch is off",
            next: "fix the underlying failure, then press `R` to reset the circuit",
        },
    ),
    (
        // `daemon::discord_events::record` — a worker inbox report of kind
        // `blocked`, with no `reason` on the report itself.
        "worker_blocked",
        Remedy {
            step: Move::Answer,
            means: "the worker reported it is blocked",
            next: "press Enter and tell it how to proceed",
        },
    ),
    (
        // `monitor::apply::apply_inbox` — the same report, read through the
        // monitor pass rather than the Discord event mirror.
        "worker blocked",
        Remedy {
            step: Move::Answer,
            means: "the worker reported it is blocked",
            next: "press Enter and tell it how to proceed",
        },
    ),
    (
        // `overseer::session::SessionResult::TimedOut`, read through triage.
        "triage session timed out",
        Remedy {
            step: Move::Review,
            means: "the triage session did not respond within its timeout",
            next: "check the triage backend is reachable, then re-run triage on the task",
        },
    ),
    (
        // `overseer::session::SessionResult::Missing`, read through triage.
        "triage session exited without result.json",
        Remedy {
            step: Move::Review,
            means: "the triage session exited without writing a result",
            next: "check the triage session log, then re-run triage on the task",
        },
    ),
];

pub(super) const PREFIX: &[(&str, Remedy)] = &[
    (
        // `dispatch::worker::spawn_candidate`, read through the failure
        // budget in `dispatch::runtime`.
        "spawn_failed:",
        Remedy {
            step: Move::Retry,
            means: "the daemon failed to spawn a worker for this candidate",
            next: "check the spawn error and the repository's worktree state, \
                   then let the next dispatch pass retry it",
        },
    ),
    (
        // `review::findings::circuit` — the failure circuit is open; the
        // review pass re-escalates this every pass while it stays open.
        "circuit_open:",
        Remedy {
            step: Move::Reset,
            means: "the failure circuit is open; dispatch has stopped",
            next: "fix the underlying failure, then press `R` to reset the circuit",
        },
    ),
    (
        // `review::findings::circuit` — consecutive failures are one away
        // from tripping the circuit, but nothing has broken yet.
        "circuit_at_risk:",
        Remedy {
            step: Move::Watch,
            means: "consecutive failures are approaching the circuit threshold",
            next: "nothing to do yet unless it trips",
        },
    ),
    (
        // `triage::completion::normalize` (`ParseError::RejectedAction`) — the
        // triage action itself was rejected before it could run.
        "rejected triage action:",
        Remedy {
            step: Move::Review,
            means: "a triage action was rejected before it could run",
            next: "check the triage action against the task's current state",
        },
    ),
    (
        // `triage::completion::apply_completion` — the parsed action ran and
        // itself returned an error.
        "triage action failed:",
        Remedy {
            step: Move::Review,
            means: "a triage action ran and failed",
            next: "check the triage backend logs, then re-run triage on the task",
        },
    ),
    (
        // `triage::completion::normalize` (`SessionResult::LaunchFailed`) —
        // the triage session never started.
        "triage session failed:",
        Remedy {
            step: Move::Review,
            means: "the triage session failed to launch",
            next: "check the triage backend, then re-run triage on the task",
        },
    ),
    (
        // `triage::completion::normalize` (`ParseError::Malformed`) — the
        // session ran but its `result.json` did not parse.
        "malformed result.json:",
        Remedy {
            step: Move::Review,
            means: "the session's result did not parse",
            next: "check the session log for what it actually wrote",
        },
    ),
    (
        // `session::auth::REASON` — the credential, not the work, is what
        // failed; every surface that reads this reason shares one string.
        "session_auth_failed:",
        Remedy {
            step: Move::Review,
            means: "the session could not authenticate",
            next: "check the credentials for the worker, triage, or review backend, then retry",
        },
    ),
    (
        // `review::findings::stalls` (dropr:460) — a live ledger entry has
        // not reached a terminal phase in twice `stuck_after_mins`; nothing
        // failed, it simply never advanced.
        "stalled:",
        Remedy {
            step: Move::Review,
            means: "this entry has not advanced in twice the usual stuck window",
            next: "open the task or pull request directly to see what stage it is stuck in",
        },
    ),
];
