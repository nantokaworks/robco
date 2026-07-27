//! How a session outcome becomes a judgment. Split out of `tests.rs` so both
//! files stay inside the source-size limit.

use super::tests::{dispatch_request, merge_request};
use super::{completion, result};
use crate::overseer::session::SessionResult;

#[test]
fn an_authentication_refusal_is_named_rather_than_called_a_fail_safe() {
    // The pull request is still escalated — a session that never ran produced no
    // opinion — but the reason has to say which of the two happened, or the
    // decision log reads the same whether the model was confused or the daemon
    // simply had no credential.
    let result::Parsed::Merge(advice) = completion::normalize(
        SessionResult::AuthFailed("Failed to authenticate: OAuth session expired".into()),
        &merge_request(),
    ) else {
        panic!("wrong advice type");
    };
    assert_eq!(
        advice.reason,
        "session_auth_failed: Failed to authenticate: OAuth session expired"
    );
    assert!(advice.fail_safe);
    assert_eq!(advice.outcome, result::MergeJudgment::Escalate);

    let result::Parsed::Dispatch(advice) = completion::normalize(
        SessionResult::AuthFailed("Invalid API key".into()),
        &dispatch_request(),
    ) else {
        panic!("wrong advice type");
    };
    assert_eq!(advice.reason, "session_auth_failed: Invalid API key");
    assert!(advice.fail_safe);
}

#[test]
fn an_ordinary_session_failure_keeps_the_generic_fail_safe_wording() {
    let result::Parsed::Merge(advice) =
        completion::normalize(SessionResult::TimedOut, &merge_request())
    else {
        panic!("wrong advice type");
    };
    assert_eq!(advice.reason, "judgment fail-safe: session timed out");
}

#[test]
fn every_dispatch_session_failure_keeps_deterministic_order() {
    let failures = [
        SessionResult::TimedOut,
        SessionResult::Missing,
        SessionResult::LaunchFailed("no executable".into()),
        SessionResult::Result(b"not json".to_vec()),
        SessionResult::Result(br#"{"candidate_ids":["unknown"],"reason":"x"}"#.to_vec()),
    ];
    for failure in failures {
        let result::Parsed::Dispatch(advice) = completion::normalize(failure, &dispatch_request())
        else {
            panic!("wrong advice type");
        };
        assert_eq!(advice.candidate_ids, ["a", "b"]);
        assert!(advice.fail_safe);
    }
}

#[test]
fn every_merge_session_failure_escalates() {
    let failures = [
        SessionResult::TimedOut,
        SessionResult::Missing,
        SessionResult::LaunchFailed("no executable".into()),
        SessionResult::Result(b"not json".to_vec()),
        SessionResult::Result(br#"{"outcome":"force","reason":"x"}"#.to_vec()),
    ];
    for failure in failures {
        let result::Parsed::Merge(advice) = completion::normalize(failure, &merge_request()) else {
            panic!("wrong advice type");
        };
        assert_eq!(advice.outcome, result::MergeJudgment::Escalate);
        assert!(advice.fail_safe);
    }
}
