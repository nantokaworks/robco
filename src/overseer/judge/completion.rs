use super::{
    Request,
    result::{self, DispatchAdvice, MergeAdvice, MergeJudgment, Parsed},
};
use crate::overseer::session::{SessionResult, auth};

pub(super) fn normalize(result: SessionResult, request: &Request) -> Parsed {
    match request {
        Request::Dispatch { approved, .. } => {
            let ids = approved
                .iter()
                .map(|item| item.task_id.clone())
                .collect::<Vec<_>>();
            match result {
                SessionResult::Result(raw) => result::parse_dispatch(&raw, &ids)
                    .map(Parsed::Dispatch)
                    .unwrap_or_else(|error| {
                        dispatch_fail_safe(ids, parse_failed(format!("{error:?}")))
                    }),
                other => dispatch_fail_safe(ids, failure_reason(other)),
            }
        }
        Request::Merge { .. } => match result {
            SessionResult::Result(raw) => result::parse_merge(&raw)
                .map(Parsed::Merge)
                .unwrap_or_else(|error| merge_fail_safe(parse_failed(format!("{error:?}")))),
            other => merge_fail_safe(failure_reason(other)),
        },
    }
}

fn dispatch_fail_safe(ids: Vec<String>, reason: String) -> Parsed {
    Parsed::Dispatch(DispatchAdvice {
        candidate_ids: ids,
        reason,
        fail_safe: true,
        ignored_fields: Vec::new(),
    })
}

fn merge_fail_safe(reason: String) -> Parsed {
    Parsed::Merge(MergeAdvice {
        outcome: MergeJudgment::Escalate,
        reason,
        fail_safe: true,
        ignored_fields: Vec::new(),
    })
}

/// A parse failure is still a judgment the session produced, so it keeps the
/// generic wording. An authentication refusal is not: the session never ran,
/// and naming it `judgment fail-safe` is what left an operator reading the
/// decision log with no way to tell a broken credential from a confused model.
fn parse_failed(error: String) -> String {
    format!("judgment fail-safe: {error}")
}

fn failure_reason(result: SessionResult) -> String {
    match result {
        SessionResult::TimedOut => parse_failed("session timed out".into()),
        SessionResult::Missing => parse_failed("session exited without result.json".into()),
        SessionResult::AuthFailed(detail) => format!("{}: {detail}", auth::REASON),
        SessionResult::LaunchFailed(error) => parse_failed(format!("session failed: {error}")),
        SessionResult::Result(_) => unreachable!(),
    }
}
