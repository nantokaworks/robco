//! `IndicatorState::merged`'s effect on a `Status::Dead` row (dropr:563).
//! Split out of `indicator_tests.rs` to keep that file at its size limit.

use super::*;

fn dead_state() -> IndicatorState {
    let mut state = IndicatorState::with_status(Some(Status::Dead));
    state.dead = true;
    state
}

#[test]
fn a_dead_session_whose_pull_request_merged_shows_plain_done_not_dead() {
    // A session the ledger has observed merged (through robco's own merge
    // flow, or externally via `gh pr merge` / github.com) is genuinely
    // finished, not an error, however its tmux session ended.
    let mut state = dead_state();
    state.merged = true;
    assert_eq!(select(state), Some(Indicator::Status(Status::Done)));
}

#[test]
fn an_unmerged_dead_session_still_shows_the_error_glyph() {
    // The other half of the acceptance criteria: a session that died without
    // the ledger ever observing a merge must keep reading as an error,
    // exactly as before.
    let mut state = dead_state();
    state.merged = false;
    assert_eq!(select(state), Some(Indicator::Status(Status::Dead)));
}

#[test]
fn a_merged_dead_row_being_auto_cleaned_up_still_shows_the_merge_spinner() {
    // While the automatic `CleanOnly` sweep is actually running against a
    // merged-and-dead row, the in-flight spinner outranks the plain `Done`
    // glyph the same way it outranks the ordinary `Dead` glyph.
    let mut state = dead_state();
    state.merged = true;
    state.merging = true;
    assert_eq!(select(state), Some(Indicator::Merging));
}
