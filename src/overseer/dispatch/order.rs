//! The deterministic ordering the dispatch gate applies to its candidates.
//!
//! Without one, "which ready task runs first" was whatever order the registry
//! happened to list repositories in, and the only thing that could impose an
//! order was the LLM judge. That made the judge load-bearing for something the
//! gate can decide by itself, and left a judge-less daemon dispatching
//! arbitrarily.
//!
//! The order is: highest priority first, then highest `priority_score` first,
//! then oldest task first. It is applied *before* the capacity gates in
//! [`super::apply_candidate_gates`], so priority decides who wins the last
//! worker slot rather than merely who spawns first; and it is the same list a
//! judge round receives, so a judge reorders a defined baseline instead of an
//! arbitrary one.

use super::Candidate;

/// Sort key for one candidate. Lower sorts earlier.
type OrderKey = (u8, u32, u8, u64, String);

/// Returns the candidates in dispatch order. See the module comment.
pub(crate) fn order_candidates(candidates: &[Candidate]) -> Vec<Candidate> {
    let mut ordered = candidates.to_vec();
    ordered.sort_by_key(order_key);
    ordered
}

fn order_key(candidate: &Candidate) -> OrderKey {
    let (numbered, age) = age_key(&candidate.display_id);
    (
        priority_rank(&candidate.priority),
        reversed_score(&candidate.priority, candidate.priority_score),
        numbered,
        age,
        // Total order: two tasks may share a priority, a score, and an
        // unparsable display id, and a sort that still depends on input order
        // is not deterministic.
        candidate.task_id.clone(),
    )
}

/// dropr's priority vocabulary, ranked. An unrecognised or missing priority
/// ranks last: an unknown value is not evidence of urgency.
fn priority_rank(priority: &str) -> u8 {
    match priority.trim().to_ascii_lowercase().as_str() {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    }
}

/// dropr's own bucket defaults for `priority_score`, mirrored so a candidate
/// that carries no score of its own resolves to the same value dropr would
/// have reported for it. An unrecognised priority (see [`priority_rank`],
/// which already sorts it last on the bucket axis) has no dropr default; `0`
/// is a placeholder that never matters once the bucket axis has ranked it.
fn bucket_default_score(priority: &str) -> u32 {
    match priority.trim().to_ascii_lowercase().as_str() {
        "high" => 300,
        "medium" => 200,
        "low" => 100,
        _ => 0,
    }
}

/// The score component of [`OrderKey`], inverted so ascending sort still reads
/// as "most urgent first": a higher `priority_score` must produce a *lower*
/// key. A missing score, or one that does not fit a `u32` (negative, or
/// larger than `u32::MAX`), falls back to [`bucket_default_score`] rather than
/// sorting the candidate ahead of or behind every scored peer in its bucket.
fn reversed_score(priority: &str, score: Option<i64>) -> u32 {
    let effective = score
        .and_then(|score| u32::try_from(score).ok())
        .unwrap_or_else(|| bucket_default_score(priority));
    u32::MAX - effective
}

/// Age proxy taken from the dropr display id.
///
/// dropr assigns `#N` in creation order within a workspace, so ascending `N` is
/// oldest-first — the only creation-time signal the ready feed carries, which
/// reports no timestamp. Ids from different workspaces are separate sequences,
/// so across repositories this is a stable tiebreak rather than a true age; the
/// gate's one-new-worker-per-repository rule already keeps repositories from
/// competing for more than one slot per pass.
///
/// Returns `(0, n)` for a parsable id and `(1, 0)` otherwise, so an unnumbered
/// candidate sorts after every numbered one instead of ahead of all of them.
fn age_key(display_id: &str) -> (u8, u64) {
    display_id
        .trim()
        .trim_start_matches('#')
        .parse::<u64>()
        .map_or((1, 0), |number| (0, number))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(display_id: &str, priority: &str) -> Candidate {
        Candidate {
            task_id: format!("task-{display_id}"),
            display_id: display_id.into(),
            title: "task".into(),
            repo: "/repo".into(),
            author: "allowed".into(),
            priority: priority.into(),
            workspace: "workspace-1".into(),
            priority_score: None,
        }
    }

    fn scored(display_id: &str, priority: &str, score: i64) -> Candidate {
        Candidate {
            priority_score: Some(score),
            ..candidate(display_id, priority)
        }
    }

    fn ordered_ids(candidates: &[Candidate]) -> Vec<String> {
        order_candidates(candidates)
            .into_iter()
            .map(|candidate| candidate.display_id)
            .collect()
    }

    #[test]
    fn priority_outranks_age() {
        let candidates = [
            candidate("#10", "low"),
            candidate("#99", "high"),
            candidate("#50", "medium"),
        ];
        assert_eq!(ordered_ids(&candidates), ["#99", "#50", "#10"]);
    }

    #[test]
    fn equal_priority_dispatches_the_oldest_first() {
        let candidates = [
            candidate("#210", "medium"),
            candidate("#7", "medium"),
            candidate("#86", "medium"),
        ];
        assert_eq!(ordered_ids(&candidates), ["#7", "#86", "#210"]);
    }

    #[test]
    fn an_unknown_priority_never_jumps_a_stated_one() {
        let candidates = [
            candidate("#1", ""),
            candidate("#900", "low"),
            candidate("#2", "unheard-of"),
        ];
        assert_eq!(ordered_ids(&candidates), ["#900", "#1", "#2"]);
    }

    #[test]
    fn an_unnumbered_candidate_sorts_last_and_stays_deterministic() {
        let candidates = [
            candidate("draft", "medium"),
            candidate("#4", "medium"),
            candidate("also-draft", "medium"),
        ];
        assert_eq!(ordered_ids(&candidates), ["#4", "also-draft", "draft"]);
    }

    #[test]
    fn ordering_is_independent_of_input_order() {
        let forward = [
            candidate("#3", "high"),
            candidate("#1", "low"),
            candidate("#2", "medium"),
        ];
        let mut reversed = forward.to_vec();
        reversed.reverse();
        assert_eq!(ordered_ids(&forward), ordered_ids(&reversed));
    }

    #[test]
    fn a_higher_score_dispatches_first_within_the_same_bucket() {
        let candidates = [
            scored("#1", "medium", 100),
            scored("#2", "medium", 250),
            scored("#3", "medium", 180),
        ];
        assert_eq!(ordered_ids(&candidates), ["#2", "#3", "#1"]);
    }

    #[test]
    fn a_scoreless_candidate_sorts_at_its_bucket_default() {
        let candidates = [
            scored("#1", "medium", 250), // above the medium default of 200
            candidate("#2", "medium"),   // no score: falls back to 200
            scored("#3", "medium", 150), // below the medium default
        ];
        assert_eq!(ordered_ids(&candidates), ["#1", "#2", "#3"]);
    }

    #[test]
    fn the_bucket_still_outranks_any_score() {
        let candidates = [scored("#1", "medium", 999), scored("#2", "high", 1)];
        assert_eq!(ordered_ids(&candidates), ["#2", "#1"]);
    }

    #[test]
    fn an_unparsable_score_falls_back_to_the_bucket_default() {
        let candidates = [
            scored("#1", "medium", -5),
            scored("#2", "medium", i64::MAX),
            candidate("#3", "medium"),
        ];
        // A negative score and one that overflows `u32` are both unparsable,
        // so they land exactly where the scoreless candidate does: the bucket
        // default. The tie is then broken by age, oldest first.
        assert_eq!(ordered_ids(&candidates), ["#1", "#2", "#3"]);
    }
}
