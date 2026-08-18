use super::*;

#[test]
fn an_empty_reason_still_yields_usable_review_guidance() {
    for reason in ["", "   "] {
        let remedy = for_reason(reason);
        assert_eq!(remedy.step, Move::Review);
        assert!(!remedy.means.is_empty());
        assert!(!remedy.next.is_empty());
    }
}

#[test]
fn resolve_downgrades_an_answer_to_review_without_a_live_session() {
    assert_eq!(resolve("merge_state:dirty", false).step, Move::Review);
    assert_eq!(resolve("merge_state:dirty", true).step, Move::Answer);
}

#[test]
fn an_orphaned_answer_is_still_actionable() {
    // An escalation with no live session is actionable whenever its move is
    // not an answer — and an answer without a session is exactly the case
    // `resolve` turns into something other than an answer.
    let remedy = resolve("merge_state:dirty", false);
    assert!(remedy.actionable());
}

#[test]
fn checks_waiting_is_never_actionable_even_with_a_live_session() {
    assert!(!resolve("checks_waiting", true).actionable());
    assert!(!resolve("checks_waiting", false).actionable());
}

#[test]
fn a_move_that_is_not_an_answer_ignores_session_state() {
    // `autonomy_envelope` resolves to `Merge`, which ORPHANED never touches:
    // merging by hand needs no tmux session either way.
    assert_eq!(resolve("autonomy_envelope", false).step, Move::Merge);
    assert_eq!(resolve("autonomy_envelope", true).step, Move::Merge);
}

#[test]
fn a_cap_wrapper_promotes_a_watch_into_a_review() {
    let promoted = for_reason("merge_hold_cap_reached:checks_waiting");
    assert_eq!(promoted.step, Move::Review);
    assert_ne!(
        promoted.means,
        for_reason("checks_waiting").means,
        "a promoted remedy must explain the automated path gave up, not just repeat \
         the inner reason's own wording"
    );
}

#[test]
fn a_disabled_recovery_wrapper_promotes_a_watch_into_a_review() {
    let promoted = for_reason("merge_recovery_disabled:checks_waiting");
    assert_eq!(promoted.step, Move::Review);
}

#[test]
fn a_disabled_recovery_wrapper_leaves_a_non_watch_move_unchanged() {
    let promoted = for_reason("merge_recovery_disabled:merge_state:dirty");
    assert_eq!(promoted.step, Move::Answer);
}

#[test]
fn a_multi_level_wrapped_reason_never_resolves_to_watch() {
    // Nested wrappers (a repeated hold around a disabled-recovery skip around
    // an unrecognised code) must still bottom out in something actionable —
    // an automated path gave up at every layer, so `Watch` ("nothing to do")
    // can never survive the promotion chain.
    let reason = "repeating_hold: 3× merge_recovery_disabled:some_unrecognised_code";
    let remedy = for_reason(reason);
    assert_ne!(remedy.step, Move::Watch);
    assert_eq!(remedy.step, Move::Review);
}

#[test]
fn a_repeat_count_is_only_stripped_when_it_actually_parses() {
    // "many" is not a count, so this must not be mistaken for the
    // `repeating_hold: <n>× ` wrapper. Nothing unwraps it, and the text ahead
    // of the first `:` (`repeating_hold`) is itself snake_case, so the prose
    // test reads the whole thing as an unrecognised code rather than free
    // text — the safe operator-review default, not a silently lost prefix.
    let remedy = for_reason("repeating_hold: many× something odd");
    assert_eq!(remedy.step, Move::Review);
}

#[test]
fn repeating_failure_unwraps_to_its_inner_reasons_own_remedy() {
    let remedy = for_reason("repeating_failure: 3× spawn_failed:io error");
    assert_eq!(remedy.step, Move::Retry);
}

#[test]
fn circuit_shapes_from_the_review_pass_resolve_distinctly() {
    assert_eq!(
        for_reason("circuit_open: 3/3 consecutive failures; latest spawn_failed:x").step,
        Move::Reset
    );
    assert_eq!(
        for_reason("circuit_at_risk: 2/3 consecutive failures; latest spawn_failed:x").step,
        Move::Watch
    );
}

#[test]
fn rejected_triage_action_resolves_to_review() {
    assert_eq!(
        for_reason("rejected triage action: unknown action kind").step,
        Move::Review
    );
}

#[test]
fn a_free_text_sentence_with_no_colon_is_still_read_as_a_verdict() {
    assert_eq!(
        for_reason("touches the migration registry without a rollback").step,
        Move::Answer
    );
}
