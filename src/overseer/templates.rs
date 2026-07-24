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
}
