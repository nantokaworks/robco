use std::path::PathBuf;

use crate::model::HostLabel;

use super::inbox;
use super::text_input::TextInput;

pub(in crate::ui) enum Mode {
    Normal,
    Help {
        scroll: u16,
    },
    PromptAgent {
        repo: usize,
        input: TextInput,
    },
    PromptRepo {
        input: TextInput,
    },
    /// Renames a repository's own directory. `path` is the pre-rename path,
    /// used to find the row again on submit — the operator's `input` may not
    /// have finished typing by the time another process changes the registry.
    PromptRenameRepo {
        path: PathBuf,
        input: TextInput,
    },
    PromptHostConnect {
        input: TextInput,
    },
    PromptOverseer {
        input: TextInput,
    },
    /// Send a one-line instruction into a repo/agent/orphan row's live
    /// CLAUDE/CODEX tmux session (dropr:565), opened by `i` while that tab is
    /// showing. `host` pins a remote control prompt to the host selected when
    /// it opened; `None` preserves local/session-derived routing.
    PromptSession {
        session: String,
        host: Option<HostLabel>,
        input: TextInput,
    },
    /// The answer prompt carries the whole row it was opened for, not just the
    /// target session: on a successful send the row's `(kind, target_id, at)`
    /// identity is what marks it handled (`App::answer_inbox`), and the
    /// identity must be the one the operator was looking at, not whatever a
    /// later refresh re-derived under the prompt.
    PromptInbox {
        item: inbox::InboxItem,
        input: TextInput,
    },
    ConfirmKill {
        repo: usize,
        agent: usize,
    },
    ConfirmRemoveRepo {
        path: PathBuf,
    },
    ConfirmMerge {
        repo: usize,
        agent: usize,
        plan: LandPlan,
        head: Option<String>,
    },
    /// The agent's pull request already merged, so `m` offers the cleanup that
    /// normally follows a merge instead of a merge that has nothing left to do.
    ConfirmCleanup {
        repo: usize,
        agent: usize,
    },
    /// Cancellable progress modal shown while `PrPrecheckJob` runs in the
    /// background, so pressing P never flashes `ConfirmPr` open only to have
    /// the precheck close it again a frame later.
    PrPrecheck {
        repo_path: PathBuf,
        agent_id: String,
        branch: String,
        approval_head: Option<String>,
    },
    ConfirmPr {
        repo_path: PathBuf,
        agent_id: String,
        branch: String,
        input: TextInput,
        approval_head: Option<String>,
    },
    ConfirmDeleteBranch {
        repo: usize,
        agent: usize,
    },
    /// `C` on a repo row: send the configured clear command to that repo's
    /// own main-worktree chat session. Confirmed like `ConfirmKill` — the
    /// conversation it discards cannot be recovered (dropr:550). Holds the
    /// repo's `path`, not its index — the same reason `ConfirmRemoveRepo`
    /// does: the row order is not stable across a background discovery
    /// refresh, and there is no per-agent identity here to re-point through
    /// like `registry_sync::dialog_agent` gives the `(repo, agent)` dialogs.
    ConfirmClearChat {
        path: PathBuf,
    },
    ErrorDialog {
        title: String,
        lines: Vec<String>,
        force_kill: Option<ForceKillTarget>,
    },
    // Holds the session NAME, not an index into `App::orphans` — the orphan
    // list is rebuilt on every discovery tick, so an index captured when the
    // dialog opened could point at a different session by the time the user
    // confirms. The name pins the kill to exactly what the dialog displayed.
    ConfirmKillOrphan {
        session: String,
    },
    // Panic-stop the overseer: kill every overseer-managed worker. Reachable
    // only while an OVERSEER row is selected.
    ConfirmOverseerPanic,
    /// Durably stop the Overseer daemon process itself (launchd bootout, or a
    /// manual SIGTERM for a daemon started with `robco daemon`) — unlike
    /// `ConfirmOverseerPanic`, this ends the daemon process, not just its
    /// workers. Reachable only while the overseer panel is visible and the
    /// daemon is alive.
    ConfirmDaemonStop,
    /// Clear every listed Inbox row. Holds the count the dialog was opened with
    /// so the prompt states what it is about to do; the rows themselves are read
    /// again on confirmation.
    ConfirmInboxDismissAll {
        count: usize,
    },
    /// Read-only view of one dropr task's full body (dropr:501), opened by
    /// `Enter` on a task-list row while `DroprTaskFocus` is focused. Drawn as
    /// a dialog (`ui::dialog::task_body`) over the task list, which stays
    /// untouched underneath — closing this (`Esc`/`h`/`Left`) returns to the
    /// exact list cursor and scroll position it had before the body opened.
    /// `scroll` is this dialog's own paragraph scroll, independent of the
    /// list pane's `preview_scroll`.
    TaskBody {
        task: usize,
        scroll: u16,
    },
    /// Delete a retained Discord channel record. Holds the channel id (not an
    /// index) since the row order re-derives from `last_active_at` on every
    /// refresh — the same hazard `ConfirmKillOrphan` guards against. `label`
    /// is the display label the dialog was opened with, reused for the
    /// result message so it reads the same even if the record is gone by the
    /// time the operator confirms.
    ConfirmRemoveDiscordChannel {
        channel_id: String,
        label: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LandPlan {
    MergeNow,
    QueueApproval,
    OpenPrThenQueue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) struct ForceKillTarget {
    pub(in crate::ui) repo_path: PathBuf,
    pub(in crate::ui) agent_id: String,
}
