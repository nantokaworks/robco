use super::{
    Request,
    result::{self, DispatchAdvice, MergeAdvice, MergeJudgment, Parsed},
};
use crate::overseer::session::SessionResult;

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
                    .unwrap_or_else(|error| dispatch_fail_safe(ids, format!("{error:?}"))),
                other => dispatch_fail_safe(ids, failure_reason(other)),
            }
        }
        Request::Merge { .. } => match result {
            SessionResult::Result(raw) => result::parse_merge(&raw)
                .map(Parsed::Merge)
                .unwrap_or_else(|error| merge_fail_safe(format!("{error:?}"))),
            other => merge_fail_safe(failure_reason(other)),
        },
    }
}

fn dispatch_fail_safe(ids: Vec<String>, reason: String) -> Parsed {
    Parsed::Dispatch(DispatchAdvice {
        candidate_ids: ids,
        reason: format!("judgment fail-safe: {reason}"),
        fail_safe: true,
    })
}

fn merge_fail_safe(reason: String) -> Parsed {
    Parsed::Merge(MergeAdvice {
        outcome: MergeJudgment::Escalate,
        reason: format!("judgment fail-safe: {reason}"),
        fail_safe: true,
    })
}

fn failure_reason(result: SessionResult) -> String {
    match result {
        SessionResult::TimedOut => "session timed out".into(),
        SessionResult::Missing => "session exited without result.json".into(),
        SessionResult::LaunchFailed(error) => format!("session failed: {error}"),
        SessionResult::Result(_) => unreachable!(),
    }
}
