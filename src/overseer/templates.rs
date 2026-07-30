use crate::{config::language_directive, overseer::OVERSEER_AGENT_ID};

pub fn worker_prompt(
    display_id: &str,
    task_id: &str,
    title: &str,
    repo: &str,
    language: Option<&str>,
) -> String {
    let directive = language_directive(language);
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

If, while implementing, you discover this task cannot proceed until a *different* dropr task
merges first, that is an ordering wait, not a blocker: do NOT mark this task `blocked`, and do NOT
run `robco report --kind blocked`. Instead run
`dropr task dependency create --task {task_id} --depends-on <prerequisite-task> --kind blocks`,
then immediately `robco report --kind waiting-prerequisite`, then release your claim (set this
task's dropr status back to `open`), and stop. dropr excludes a task behind an unresolved `blocks`
edge from its own ready feed, so Overseer redispatches automatically once the prerequisite merges —
no operator action, and nothing further for you to do here.

{directive}Never merge. Never force push. Never push to main. Never create extra worktrees."#
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
    language: Option<&str>,
) -> String {
    let directive = language_directive(language);
    format!(
        r#"Overseer could not merge the pull request for Dropr task {display_id} ({task_id}).
Pull request: {pr_url}
Failure reason (verbatim, this is your instruction): {reason}

Fix it on the branch you were already assigned — do not create a new branch or worktree.
Resolve the failure the reason names (a conflict with the base, a red check, a reviewer
veto), re-run the relevant tests, commit with `(refs dropr:{task_id})` in the message, and
push the same branch. Then run `robco report --kind done`. Overseer re-evaluates the pull
request on its next pass and merges it once the gate is satisfied.

{directive}Never merge. Never force push. Never push to main. Never create extra worktrees.
You fix the branch; Overseer merges it."#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(language: Option<&str>) -> String {
        worker_prompt("#132", "abc", "Build overseer", "/repo", language)
    }

    fn recovery(language: Option<&str>) -> String {
        merge_recovery_prompt("#132", "abc", "https://pr/1", "merge_state:dirty", language)
    }

    #[test]
    fn prompt_contains_assignment_and_rails() {
        let prompt = worker(None);
        assert!(prompt.contains("Close Dropr: #132"));
        assert!(prompt.contains("(refs dropr:abc)"));
        assert!(prompt.contains("Never merge"));
    }

    #[test]
    fn prompt_hands_the_claim_over_instead_of_asking_for_one() {
        // The overseer claims at dispatch time; a worker that re-claimed would be
        // racing its own dispatcher for the lock it already benefits from.
        let prompt = worker(None);
        assert!(prompt.contains("already claimed #132"));
        assert!(prompt.contains("Do NOT run `dropr task next`"));
    }

    /// The rails are the last thing either prompt says, so the directive goes in
    /// above them rather than after — see the placement assertions below.
    #[test]
    fn a_configured_language_reaches_both_prompts_above_the_rails() {
        for prompt in [worker(Some("Japanese")), recovery(Some("Japanese"))] {
            let directive = prompt
                .find("LANGUAGE: ")
                .expect("the directive is rendered");
            let rails = prompt.find("Never merge").expect("the rails survive");
            assert!(directive < rails, "{prompt}");
            assert!(prompt.contains("in Japanese."), "{prompt}");
        }
    }

    /// The guarantee a config without the key rests on.
    #[test]
    fn an_unset_language_leaves_both_prompts_byte_identical() {
        assert_eq!(worker(Some("   ")), worker(None));
        assert_eq!(recovery(Some("   ")), recovery(None));
        assert!(!worker(None).contains("LANGUAGE: "));
        assert!(!recovery(None).contains("LANGUAGE: "));
    }

    #[test]
    fn recovery_prompt_carries_the_reason_verbatim_and_the_rails() {
        let prompt = merge_recovery_prompt(
            "#132",
            "abc",
            "https://pr/1",
            "judge_veto:touches the migration registry without a rollback",
            None,
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
        let prompt = recovery(None);
        assert!(prompt.contains("do not create a new branch or worktree"));
        assert!(prompt.contains("push the same branch"));
    }
}
