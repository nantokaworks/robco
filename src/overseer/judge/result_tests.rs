//! What a verdict has to say before it is believed, and what it may say freely.
//!
//! The dividing line is meaning: an `outcome` the gate cannot act on and a
//! `reason` that explains nothing are refused, while a key the schema never
//! asked for is ignored and named.

use super::*;

/// The verdict PR #190 was actually escalated over, byte for byte. It is an
/// `allow` with a `verification` object the judge added on its own initiative,
/// and `deny_unknown_fields` turned it into a fail-safe escalation.
const ALLOW_WITH_VERIFICATION: &[u8] =
    include_bytes!("fixtures/merge_allow_with_verification.json");

#[test]
fn the_verdict_that_was_escalated_over_an_extra_key_now_allows_the_merge() {
    let advice = parse_merge(ALLOW_WITH_VERIFICATION).unwrap();
    assert_eq!(advice.outcome, MergeJudgment::Allow);
    assert!(!advice.fail_safe);
    assert!(advice.reason.starts_with("Verified the PR head"));
    assert_eq!(advice.ignored_fields, ["verification"]);
}

#[test]
fn an_extra_key_alongside_a_veto_is_ignored_the_same_way() {
    let advice =
        parse_merge(br#"{"outcome":"veto","reason":"no rollback","verification":{"a":1}}"#)
            .unwrap();
    assert_eq!(advice.outcome, MergeJudgment::Veto);
    assert!(!advice.fail_safe);
    assert_eq!(advice.ignored_fields, ["verification"]);
}

/// The ignored keys are reported in sorted order so the same verdict always
/// reads the same way in `decisions.jsonl`, whatever order the model wrote them.
#[test]
fn ignored_keys_are_reported_in_a_stable_order() {
    let advice = parse_merge(br#"{"outcome":"allow","reason":"ok","zeta":1,"alpha":2}"#).unwrap();
    assert_eq!(advice.ignored_fields, ["alpha", "zeta"]);
}

/// The fail-safe still covers every verdict whose *meaning* is unusable.
#[test]
fn an_unknown_outcome_or_an_empty_reason_is_still_malformed() {
    for raw in [
        &br#"{"outcome":"force","reason":"x"}"#[..],
        &br#"{"outcome":"allow","reason":" "}"#[..],
        &br#"{"outcome":"allow"}"#[..],
        &br#"{"reason":"x"}"#[..],
    ] {
        assert!(
            matches!(parse_merge(raw), Err(ParseError::Malformed(_))),
            "expected malformed: {}",
            String::from_utf8_lossy(raw)
        );
    }
}

/// The dispatch round has the same trust model, so it gets the same treatment —
/// but the id allow-list is a security boundary and stays closed.
#[test]
fn a_dispatch_round_ignores_extra_keys_and_still_refuses_unapproved_ids() {
    let advice = parse_dispatch(
        br#"{"candidate_ids":["a"],"reason":"priority","notes":"extra"}"#,
        &["a".into()],
    )
    .unwrap();
    assert_eq!(advice.candidate_ids, ["a"]);
    assert_eq!(advice.ignored_fields, ["notes"]);

    assert!(matches!(
        parse_dispatch(
            br#"{"candidate_ids":["a","rejected"],"reason":"priority"}"#,
            &["a".into(), "b".into()]
        ),
        Err(ParseError::Rejected(_))
    ));
    assert!(parse_dispatch(br#"{"candidate_ids":["a"],"reason":" "}"#, &["a".into()]).is_err());
}
