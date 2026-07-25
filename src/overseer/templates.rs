use crate::overseer::OVERSEER_AGENT_ID;

pub fn worker_prompt(display_id: &str, task_id: &str, title: &str, repo: &str) -> String {
    format!(
        r#"You are the autonomous RUN worker for assigned Dropr task {display_id} ({task_id}):
{title}
Repository: {repo}

Run `dropr bootstrap` first. The Overseer already claimed {display_id} for you, as dropr agent
`{OVERSEER_AGENT_ID}` — the task is yours. Do NOT run `dropr task next` and do NOT claim it again;
a second claim would only fight the one you already hold. Verify the task is claimed by
`{OVERSEER_AGENT_ID}`, then run `robco report --kind claimed`. If it is claimed by anyone else, run
`robco report --kind blocked` and stop without touching the repository.

Follow RUN discipline: implement the task, self-review the diff, run relevant tests, commit with
`(refs dropr:{task_id})` in the commit message, and push only your assigned branch. Open a pull
request whose body contains `Close Dropr: {display_id}`. Finally run `robco report --kind done`.
The current report CLI carries lifecycle kind only; Overseer discovers the PR URL from your branch.

Never merge. Never force push. Never push to main. Never create extra worktrees."#
    )
}

/// What a worker is told when Overseer could not merge the pull request it opened.
///
/// The failure reason is passed through verbatim: for a judge veto it is the
/// judge's own words and therefore the actual instruction, and for a merge state
/// it names the state to resolve. Paraphrasing it here would put an Overseer-side
/// guess between the verdict and the worker acting on it.
///
/// The rails are restated rather than assumed. A merge failure is exactly the
/// situation where a worker is most tempted to merge the pull request itself, and
/// the original dispatch prompt may be far up its transcript by now.
pub fn merge_recovery_prompt(
    display_id: &str,
    task_id: &str,
    pr_url: &str,
    reason: &str,
) -> String {
    format!(
        r#"Overseer could not merge the pull request for Dropr task {display_id} ({task_id}).
Pull request: {pr_url}
Failure reason (verbatim, this is your instruction): {reason}

Fix it on the branch you were already assigned — do not create a new branch or worktree.
Resolve the failure the reason names (a conflict with the base, a red check, a reviewer
veto), re-run the relevant tests, commit with `(refs dropr:{task_id})` in the message, and
push the same branch. Then run `robco report --kind done`. Overseer re-evaluates the pull
request on its next pass and merges it once the gate is satisfied.

Never merge. Never force push. Never push to main. Never create extra worktrees.
You fix the branch; Overseer merges it."#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_contains_assignment_and_rails() {
        let prompt = worker_prompt("#132", "abc", "Build overseer", "/repo");
        assert!(prompt.contains("Close Dropr: #132"));
        assert!(prompt.contains("(refs dropr:abc)"));
        assert!(prompt.contains("Never merge"));
    }

    #[test]
    fn prompt_hands_the_claim_over_instead_of_asking_for_one() {
        // The overseer claims at dispatch time; a worker that re-claimed would be
        // racing its own dispatcher for the lock it already benefits from.
        let prompt = worker_prompt("#132", "abc", "Build overseer", "/repo");
        assert!(prompt.contains("already claimed #132"));
        assert!(prompt.contains("Do NOT run `dropr task next`"));
    }

    #[test]
    fn recovery_prompt_carries_the_reason_verbatim_and_the_rails() {
        let prompt = merge_recovery_prompt(
            "#132",
            "abc",
            "https://pr/1",
            "judge_veto:touches the migration registry without a rollback",
        );
        assert!(prompt.contains("judge_veto:touches the migration registry without a rollback"));
        assert!(prompt.contains("https://pr/1"));
        assert!(prompt.contains("(refs dropr:abc)"));
        for rail in [
            "Never merge",
            "Never force push",
            "Never push to main",
            "Never create extra worktrees",
        ] {
            assert!(prompt.contains(rail), "missing rail: {rail}");
        }
    }

    #[test]
    fn recovery_prompt_never_asks_for_a_new_branch() {
        // A worker that cut a second branch would strand the pull request
        // Overseer is waiting on, so the instruction has to be explicit.
        let prompt = merge_recovery_prompt("#132", "abc", "https://pr/1", "merge_state:dirty");
        assert!(prompt.contains("do not create a new branch or worktree"));
        assert!(prompt.contains("push the same branch"));
    }
}
