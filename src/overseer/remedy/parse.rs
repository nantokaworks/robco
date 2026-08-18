//! Unwrapping a reason down to the table entry that actually names it.
//!
//! A reason is not always the thing to look up directly: the review pass
//! wraps a repeated reason in `repeating_hold: <n>× ` or
//! `repeating_failure: <n>× `, and `merge_recovery` wraps a reason it
//! dispatched, skipped, or declined to hand back at all. Each wrapper is
//! stripped in turn, bounded to [`MAX_DEPTH`] layers so a malformed or
//! adversarial chain cannot recurse forever, until what is left is either a
//! table entry or plain enough for the prose test at the bottom to call.
//!
//! Some wrappers mean the automated path already gave up on whatever the
//! inner reason said, so the wrapper's own wording must win outright rather
//! than defer to the inner remedy — see [`ALWAYS_PROMOTE`] and
//! `repeating_hold`'s handling below. `repeating_failure` and
//! `merge_recovery_disabled` are different: the underlying fix is still
//! whatever the inner reason already says, so those keep (or conditionally
//! keep) the inner remedy instead.

use super::{Move, Remedy, classify_fallback, table};

/// Wrappers that carry no information of their own — the inner reason is the
/// whole story, so resolution recurses straight into it.
const UNWRAP_PREFIXES: &[&str] = &["merge_recovery_dispatched:", "merge_recovery_skipped:"];

/// A wrapper meaning an automated path gave up on the inner reason. The inner
/// reason still resolves normally, except that a `Watch` verdict — normally
/// "nothing to do" — cannot survive an automated path abandoning it, so it is
/// promoted to `Review` with wording that says so.
struct PromoteWrap {
    prefix: &'static str,
    means: &'static str,
    next: &'static str,
}

const PROMOTE_WRAPS: &[PromoteWrap] = &[PromoteWrap {
    // `daemon::merge_recovery::disabled` — a recoverable failure the
    // `merge_recovery_enabled` switch left unhanded.
    prefix: "merge_recovery_disabled:",
    means: "this looks worker-fixable, but automated handback is switched off",
    next: "press Enter and relay the reason to the worker yourself, \
           or turn on merge_recovery_enabled",
}];

/// A wrapper whose own meaning must win regardless of what the inner reason
/// says — unlike [`PROMOTE_WRAPS`], which only overrides a `Watch` verdict,
/// these mark a point where an automated path already retried the inner
/// condition and gave up, so repeating the inner reason's own wording (a
/// worker fix, a wait, whatever it originally suggested) would misdescribe a
/// case that is now the operator's alone.
struct AlwaysPromote {
    prefix: &'static str,
    remedy: Remedy,
}

const ALWAYS_PROMOTE: &[AlwaysPromote] = &[
    AlwaysPromote {
        // `daemon::merge_hold::cap_reached` — the same hold reason repeated
        // past the per-entry hold budget (dropr:460).
        prefix: "merge_hold_cap_reached:",
        remedy: Remedy {
            step: Move::Review,
            means: "the gate held this pull request on one condition for the full hold budget",
            next: "read the reason to see which condition stopped it, then clear that \
                   condition yourself",
        },
    },
    AlwaysPromote {
        // `daemon::merge_recovery::undeliverable` — the handback could not be
        // confirmed delivered twice for one revision (dropr:460). Task #455
        // proved this often false-negatives on a delivery that did land, and
        // #457 (PR #365) reduced but did not remove that case, so the wording
        // asks for a check rather than assuming the worker never saw it.
        prefix: "merge_recovery_undeliverable:",
        remedy: Remedy {
            step: Move::Review,
            means: "delivery to the worker could not be confirmed twice for this revision; \
                    this is often a false negative, and the worker may already be fixing it",
            next: "check the pull request and the worker's session before acting; \
                   if it really is stuck, handle it by hand",
        },
    },
];

/// The board review saw the same reason repeat past its own threshold, three
/// or more passes in a row with nothing clearing it. Static like every other
/// entry here, so it names the pattern rather than the specific reason or
/// count — dropr #461/#462 carry those as per-case facts.
const REPEATING_HOLD: Remedy = Remedy {
    step: Move::Review,
    means: "the board review saw this same condition repeat three or more times in a row; \
            nothing about the pull request is moving",
    next: "open the pull request directly; automatic retries are not resolving this on their own",
};

const MAX_DEPTH: u8 = 4;

/// A reason with nothing recorded — there is no wrapper to strip and no
/// prose to test, only the fact that the field was empty.
const EMPTY_REASON: Remedy = Remedy {
    step: Move::Review,
    means: "no reason was recorded for this escalation",
    next: "check the decision log and the pull request directly",
};

/// A free-text reason rather than a snake_case code — the words themselves
/// are the instruction, exactly like a table-matched `Answer`. Defensive
/// fallback: nothing in the vocabulary produces free-text reasons today, but
/// an unrecognised sentence-shaped reason still needs a sensible remedy
/// rather than falling through to the generic operator fallback.
const FREE_TEXT_VERDICT: Remedy = Remedy {
    step: Move::Answer,
    means: "this reason reads as free text rather than a known code",
    next: "press Enter and relay the reason to the worker, or act on it yourself",
};

/// Resolves a reason with no knowledge of whether a session is live —
/// [`resolve`] is what a caller with a session to check should use instead.
pub(crate) fn for_reason(reason: &str) -> Remedy {
    resolve_at(reason, MAX_DEPTH)
}

/// Resolves a reason, downgrading an `Answer` to [`super::ORPHANED`] when
/// there is no live session to answer into. An instruction with nowhere to
/// send it is the operator's problem, not the worker's.
pub(crate) fn resolve(reason: &str, live_session: bool) -> Remedy {
    let remedy = for_reason(reason);
    if remedy.step == Move::Answer && !live_session {
        super::ORPHANED
    } else {
        remedy
    }
}

fn resolve_at(reason: &str, depth: u8) -> Remedy {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        return EMPTY_REASON;
    }
    if let Some(remedy) = table::exact(trimmed) {
        return remedy;
    }
    if let Some(remedy) = table::prefix(trimmed) {
        return remedy;
    }
    if depth > 0 {
        for prefix in UNWRAP_PREFIXES {
            if let Some(inner) = trimmed.strip_prefix(prefix) {
                return resolve_at(inner, depth - 1);
            }
        }
        for wrap in PROMOTE_WRAPS {
            if let Some(inner) = trimmed.strip_prefix(wrap.prefix) {
                let remedy = resolve_at(inner, depth - 1);
                return if remedy.step == Move::Watch {
                    Remedy {
                        step: Move::Review,
                        means: wrap.means,
                        next: wrap.next,
                    }
                } else {
                    remedy
                };
            }
        }
        for always in ALWAYS_PROMOTE {
            if trimmed.starts_with(always.prefix) {
                return always.remedy;
            }
        }
        if strip_repeat_count(trimmed, "repeating_hold").is_some() {
            return REPEATING_HOLD;
        }
        if let Some(inner) = strip_repeat_count(trimmed, "repeating_failure") {
            return resolve_at(inner, depth - 1);
        }
    }
    if looks_like_code(trimmed) {
        classify_fallback(trimmed)
    } else {
        FREE_TEXT_VERDICT
    }
}

/// Strips a `"{label}: {n}× "` wrapper, but only when `n` actually parses as a
/// count — a label that merely starts the same way (or whose count field
/// holds something else) is left alone rather than silently mangled.
fn strip_repeat_count<'a>(reason: &'a str, label: &str) -> Option<&'a str> {
    let rest = reason.strip_prefix(label)?.strip_prefix(": ")?;
    let times_at = rest.find('×')?;
    let (count, after) = rest.split_at(times_at);
    if count.is_empty() || !count.chars().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    after.strip_prefix('×')?.strip_prefix(' ')
}

/// A reason is a code when everything before its first `:` is snake_case; a
/// sentence — spaces, punctuation, capitals — is free text instead.
fn looks_like_code(reason: &str) -> bool {
    let head = reason.split(':').next().unwrap_or(reason);
    !head.is_empty()
        && head
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
