//! Cache keys for judgment rounds.
//!
//! Each key names the exact question a round was asked, so a verdict is only
//! reused for the question it answered: a dispatch key changes the moment the
//! candidate set does, and a merge key changes the moment the change under
//! review does. [`super::completed`] exists because that is a strength — the
//! verdict left under a superseded key must be dropped and recorded, not
//! quietly reused.

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

/// Fingerprints the change under review, deliberately *not* its head commit.
///
/// The org ruleset requires branches to be up to date, so every merge into
/// `main` leaves every other open pull request behind and the gate updates it.
/// That update rewrites the head sha without touching the change: keying on the
/// head threw away a verdict the gate had just paid one to three minutes of
/// model time for, and queued an identical judgment for the new head — which
/// the next merge then invalidated in turn. On a busy board no pull request
/// ever reached a verdict it could act on.
///
/// The tradeoff this makes explicit: an `allow` computed at head H can merge
/// head H', where H' is H with the base merged in. That is sound because the
/// required status checks are re-run and must pass on H' before the gate
/// proceeds — `pull_request::checks` reads the rollup of the *current* head —
/// and because merging the base in is mechanical. The alternative, re-running a
/// multi-minute judgment on every base update, is what stopped merges from
/// happening at all.
///
/// `title` and `body` are part of the fingerprint on purpose: an edited pull
/// request description changes what the judge was asked about. The known limit
/// is a push that leaves the file list and both line counts identical — the
/// fingerprint cannot see it, and the re-run required checks are what stands
/// behind the merge in that case.
pub(super) fn merge_key(case: &MergeCase) -> String {
    let mut files = case.files.clone();
    files.sort();
    let counts = format!("{}f+{}-{}", files.len(), case.additions, case.deletions);
    stable_key(
        "merge",
        [
            case.task_id.as_str(),
            case.pr_url.as_str(),
            case.title.as_str(),
            case.body.as_str(),
            counts.as_str(),
        ]
        .into_iter()
        .chain(files.iter().map(String::as_str)),
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
