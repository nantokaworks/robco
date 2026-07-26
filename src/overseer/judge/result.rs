use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdvice {
    pub candidate_ids: Vec<String>,
    pub reason: String,
    pub fail_safe: bool,
    /// Keys the verdict carried that the schema does not name, sorted. Ignored
    /// by the parser and recorded by [`super::audit`] — see [`RawMerge`].
    pub ignored_fields: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MergeJudgment {
    Allow,
    Veto,
    Escalate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeAdvice {
    pub outcome: MergeJudgment,
    pub reason: String,
    pub fail_safe: bool,
    /// Keys the verdict carried that the schema does not name, sorted. Ignored
    /// by the parser and recorded by [`super::audit`] — see [`RawMerge`].
    pub ignored_fields: Vec<String>,
}

/// What the merge judge has to say about one pull request right now.
///
/// The three states used to collapse into `Option<MergeAdvice>`, and the two
/// that carry no verdict are not the same thing at all: a queued judgment ends
/// on its own, while a refusal the gate already holds never does. Reading both
/// as "wait" is what let an escalated pull request sit for hours with no
/// decision recorded on any pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeVerdict {
    /// The judge answered.
    Advice(MergeAdvice),
    /// A judgment is queued or running; there is no verdict yet.
    Queued,
    /// The judge already refused this exact change, so it is not asked again.
    Refused,
}

impl MergeVerdict {
    /// The verdict itself, for tests that assert on what the judge said rather
    /// than on which of the three states came back.
    #[cfg(test)]
    pub(crate) fn advice(&self) -> Option<&MergeAdvice> {
        match self {
            Self::Advice(advice) => Some(advice),
            Self::Queued | Self::Refused => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    Dispatch(DispatchAdvice),
    Merge(MergeAdvice),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Malformed(String),
    Rejected(String),
}

#[derive(Deserialize)]
struct RawDispatch {
    candidate_ids: Vec<String>,
    reason: String,
    /// See [`RawMerge`]: the dispatch round has the same trust model and the
    /// same failure mode, so it gets the same treatment.
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

/// A merge verdict exactly as the model wrote it.
///
/// Unknown keys are collected rather than refused. `deny_unknown_fields` is the
/// right instinct for a payload an attacker controls, but this one comes from a
/// local model session whose shape drift is expected: a judge that answered
/// `allow` and added a `verification` object of its own accord had the whole
/// approval thrown away and the pull request escalated. The fail-safe exists to
/// protect against a verdict that cannot be *understood*, not one that says more
/// than it was asked to.
///
/// What the parser still refuses is unchanged: an `outcome` outside
/// [`MergeJudgment`], and a `reason` that is missing or blank. The ignored keys
/// are carried out on [`MergeAdvice::ignored_fields`] so dropping the model's
/// extra work is visible in `decisions.jsonl` rather than silent.
#[derive(Deserialize)]
struct RawMerge {
    outcome: MergeJudgment,
    reason: String,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

pub(super) fn is_complete(raw: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(raw).is_ok()
}

pub(super) fn parse_dispatch(
    raw: &[u8],
    approved: &[String],
) -> Result<DispatchAdvice, ParseError> {
    let value: RawDispatch =
        serde_json::from_slice(raw).map_err(|error| ParseError::Malformed(error.to_string()))?;
    if value.reason.trim().is_empty() {
        return Err(ParseError::Malformed("reason must not be blank".into()));
    }
    let approved: HashSet<_> = approved.iter().collect();
    let mut seen = HashSet::new();
    for id in &value.candidate_ids {
        if !approved.contains(id) {
            return Err(ParseError::Rejected(format!(
                "candidate is not approved: {id}"
            )));
        }
        if !seen.insert(id) {
            return Err(ParseError::Rejected(format!("duplicate candidate: {id}")));
        }
    }
    Ok(DispatchAdvice {
        candidate_ids: value.candidate_ids,
        reason: value.reason,
        fail_safe: false,
        ignored_fields: value.extra.into_keys().collect(),
    })
}

pub(super) fn parse_merge(raw: &[u8]) -> Result<MergeAdvice, ParseError> {
    let value: RawMerge =
        serde_json::from_slice(raw).map_err(|error| ParseError::Malformed(error.to_string()))?;
    if value.reason.trim().is_empty() {
        return Err(ParseError::Malformed("reason must not be blank".into()));
    }
    Ok(MergeAdvice {
        outcome: value.outcome,
        reason: value.reason,
        fail_safe: false,
        ignored_fields: value.extra.into_keys().collect(),
    })
}

#[cfg(test)]
#[path = "result_tests.rs"]
mod tests;
