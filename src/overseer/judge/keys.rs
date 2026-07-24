//! Cache keys for judgment rounds.
//!
//! Each key names the exact question a round was asked, so a verdict is only
//! reused for the question it answered: a dispatch key changes the moment the
//! candidate set does, and a merge key changes the moment the head commit does.
//! `JudgmentQueue::discard_stale_dispatch` exists because that is a strength —
//! the stale verdict must be dropped and recorded, not quietly reused.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use super::MergeCase;
use crate::overseer::dispatch::Candidate;

pub(super) fn dispatch_key(approved: &[Candidate]) -> String {
    stable_key(
        "dispatch",
        approved.iter().map(|item| item.task_id.as_str()),
    )
}

pub(super) fn merge_key(case: &MergeCase) -> String {
    stable_key(
        "merge",
        [
            case.task_id.as_str(),
            case.pr_url.as_str(),
            case.head_sha.as_str(),
        ],
    )
}

pub(super) fn merge_identity(case: &MergeCase) -> String {
    merge_identity_parts(&case.task_id, &case.pr_url)
}

pub(super) fn merge_identity_parts(task_id: &str, pr_url: &str) -> String {
    stable_key("merge-identity", [task_id, pr_url])
}

fn stable_key<'a>(kind: &str, values: impl IntoIterator<Item = &'a str>) -> String {
    let mut hasher = DefaultHasher::new();
    kind.hash(&mut hasher);
    for value in values {
        value.hash(&mut hasher);
    }
    format!("{kind}-{:016x}", hasher.finish())
}
