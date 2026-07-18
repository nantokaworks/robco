use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdvice {
    pub candidate_ids: Vec<String>,
    pub reason: String,
    pub fail_safe: bool,
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
#[serde(deny_unknown_fields)]
struct RawDispatch {
    candidate_ids: Vec<String>,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMerge {
    outcome: MergeJudgment,
    reason: String,
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
    })
}
