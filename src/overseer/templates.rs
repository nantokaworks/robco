use crate::{config::language_directive, dropr::Subtask};

/// Default value of `overseer.worker_prompt_template` — the task-specific
/// half of [`worker_prompt`], unset. An operator's override replaces this
/// text wholesale; it never reaches the claim instruction, the "open a PR"
/// ending, or the rails, which `worker_prompt` always appends after it.
const DEFAULT_WORKER_PROMPT_TEMPLATE: &str = r#"You are the autonomous RUN worker for assigned Dropr task {display_id} ({task_id}):
{title}
Repository: {repo}

If, while implementing, you discover this task cannot proceed until a *different* dropr task
merges first, that is an ordering wait, not a blocker: do NOT mark this task `blocked`, and do NOT
run `robco report --kind blocked`. Instead run
`dropr task dependency create --task {task_id} --depends-on <prerequisite-task> --kind blocks`,
then immediately `robco report --kind waiting-prerequisite`, then release your claim (set this
task's dropr status back to `open`), and stop. dropr excludes a task behind an unresolved `blocks`
edge from its own ready feed, so Overseer redispatches automatically once the prerequisite merges —
no operator action, and nothing further for you to do here.

If you already reported `blocked` and a human then answers you directly inside this session —
not through dropr, not through the Inbox — run `robco report --kind unblocked` right away. That
tells Overseer the block lifted immediately instead of waiting for its next observation pass."#;

/// Fills `{display_id}`, `{task_id}`, `{title}`, `{repo}`, and `{subtasks}`
/// placeholders into `template` — the operator's configured
/// `worker_prompt_template`, or [`DEFAULT_WORKER_PROMPT_TEMPLATE`] when unset
/// or blank. Plain substitution, not `format!`, because the template text
/// itself is runtime data, not a compile-time literal.
fn render_task_specific(
    template: Option<&str>,
    display_id: &str,
    task_id: &str,
    title: &str,
    repo: &str,
    subtasks: &[Subtask],
) -> String {
    let raw = template
        .map(str::trim)
        .filter(|template| !template.is_empty())
        .unwrap_or(DEFAULT_WORKER_PROMPT_TEMPLATE);
    let subtasks_rendered = if subtasks.is_empty() {
        "none".to_string()
    } else {
        subtasks
            .iter()
            .map(|subtask| subtask.display_id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    raw.replace("{display_id}", display_id)
        .replace("{task_id}", task_id)
        .replace("{title}", title)
        .replace("{repo}", repo)
        .replace("{subtasks}", &subtasks_rendered)
}

/// `template` is the operator-configurable half — `overseer.worker_prompt_template`,
/// task-specific instructions on how to work, what to check, house style. Everything
/// after it in the rendered prompt — the claim instruction, the "open a PR, do not
/// merge it" ending, and the never-merge rails — is fixed text the code always
/// appends; no `template` value can remove or reach it (dropr:470).
pub fn worker_prompt(
    display_id: &str,
    task_id: &str,
    title: &str,
    repo: &str,
    subtasks: &[Subtask],
    language: Option<&str>,
    template: Option<&str>,
) -> String {
    let directive = language_directive(language);
    let close_directive = close_directive_instruction(display_id, subtasks);
    let task_specific = render_task_specific(template, display_id, task_id, title, repo, subtasks);
    format!(
        r#"{task_specific}

Run `dropr bootstrap` first. The Overseer already claimed {display_id} for you — the task is yours.
Do NOT run `dropr task next` and do NOT claim it again; a second claim would only fight the one you
already hold. Run `robco report --kind claimed`.

Follow RUN discipline: implement the task, self-review the diff, run relevant tests, commit with
`(refs dropr:{task_id})` in the commit message, and push only your assigned branch. Open a pull
request whose body contains {close_directive} Finally run `robco report --kind done`.
The current report CLI carries lifecycle kind only; Overseer discovers the PR URL from your branch.

{directive}Never merge. Never force push. Never push to main. Never change the branch of the
repository under `~/.robco/repos/` — not with `git checkout`, not with `gh pr checkout`. To
inspect another PR, make a throwaway worktree outside that managed tree (for example under
the system temp directory) and remove it when you are done — that is the one worktree this
rule allows; do not create any other extra worktree.
Never end the tmux server. You are inside a tmux session that robco shares with every other
worker and with the operator's own chat, and `TMUX_TMPDIR` does not isolate you: tmux takes the
socket path from `$TMUX` and ignores it. If you need a throwaway tmux server, unset `TMUX` and
name a short socket path of your own, for example `env -u TMUX tmux -S /tmp/probe.sock ...`."#
    )
}

/// The dispatch prompt's close-directive instruction, one line for a childless
/// task and one `Close Dropr:` line per subtask plus a `Refs Dropr:` line for
/// the parent when the dispatched task has subtasks
/// (dropr:yD5Gf6TX23VMvuSLFsmvO).
///
/// Without this, a parent dispatch's PR closed only the parent: every subtask
/// stayed `open` and unclaimed, so dropr kept offering them on the ready feed
/// and Overseer dispatched a fresh worker for work that had already merged.
fn close_directive_instruction(display_id: &str, subtasks: &[Subtask]) -> String {
    if subtasks.is_empty() {
        return format!("`Close Dropr: {display_id}`.");
    }
    let close_lines = subtasks
        .iter()
        .map(|subtask| format!("`Close Dropr: {}`", subtask.display_id))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "one line for each subtask this run covers — {close_lines} — plus \
         `Refs Dropr: {display_id}` for the parent task itself."
    )
}

/// What a worker is told when Overseer could not merge the pull request it opened.
///
/// The failure reason is passed through verbatim — for a merge state it names
/// the state to resolve, for a protection hold it says so directly.
/// Paraphrasing it here would put an Overseer-side guess between the reason
/// and the worker acting on it.
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

{directive}Never merge. Never force push. Never push to main. Never change the branch of the
repository under `~/.robco/repos/` — not with `git checkout`, not with `gh pr checkout`. To
inspect another PR, make a throwaway worktree outside that managed tree (for example under
the system temp directory) and remove it when you are done — that is the one worktree this
rule allows; do not create any other extra worktree.
Never end the tmux server. You are inside a tmux session that robco shares with every other
worker and with the operator's own chat, and `TMUX_TMPDIR` does not isolate you: tmux takes the
socket path from `$TMUX` and ignores it. If you need a throwaway tmux server, unset `TMUX` and
name a short socket path of your own, for example `env -u TMUX tmux -S /tmp/probe.sock ...`.
You fix the branch; Overseer merges it."#
    )
}

#[cfg(test)]
#[path = "templates_tests.rs"]
mod tests;
