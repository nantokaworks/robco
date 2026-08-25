use ratatui::layout::Rect;

use super::*;
use crate::{
    config::Config,
    locale::Locale,
    registry::Registry,
    ui::{LandPlan, inbox::InboxItem, inbox::InboxKind, test_support, text_input::TextInput},
};

fn item(target_id: &str, repo: Option<&str>, detail: &str) -> InboxItem {
    item_with_session(target_id, repo, detail, None)
}

fn item_with_session(
    target_id: &str,
    repo: Option<&str>,
    detail: &str,
    session: Option<&str>,
) -> InboxItem {
    InboxItem {
        kind: InboxKind::Escalation,
        repo: repo.map(ToString::to_string),
        target_session: session.map(ToString::to_string),
        target_id: target_id.into(),
        label: format!("{target_id} — {detail}"),
        detail: detail.into(),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }
}

fn dialog_app(mode: Mode, items: Vec<InboxItem>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_inbox = items;
    app.mode = mode;
    app
}

fn body(app: &App) -> String {
    let dialog = content(app, Rect::new(0, 0, 60, 30)).expect("dialog content");
    dialog
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// dropr:498 — the dialog names what it is about to hide rather than the
/// files it leaves alone.
#[test]
fn the_clear_all_dialog_names_each_item_when_there_are_few() {
    let items = vec![
        item("#159", Some("robco"), "merge_state:dirty"),
        item_with_session("agent-1", None, "checks_not_green", Some("robco-agent-1")),
    ];
    let app = dialog_app(Mode::ConfirmInboxDismissAll { count: 2 }, items);

    let text = body(&app);

    assert!(text.contains("hiding 2 item(s):"), "{text}");
    assert!(text.contains("REVIEW robco #159"), "{text}");
    assert!(text.contains("ANSWER agent-1"), "{text}");
}

/// Past the named-list threshold, the dialog summarises by remedy tag rather
/// than running the list past a dialog's height.
#[test]
fn the_clear_all_dialog_summarizes_by_tag_when_there_are_many() {
    let items = (0..7)
        .map(|index| item(&format!("#{index}"), None, "merge_state:dirty"))
        .collect();
    let app = dialog_app(Mode::ConfirmInboxDismissAll { count: 7 }, items);

    let text = body(&app);

    assert!(text.contains("hiding 7 item(s):"), "{text}");
    assert!(text.contains("7 REVIEW"), "{text}");
    // No per-item line when the list is summarised.
    assert!(!text.contains("#0"), "{text}");
}

/// The dialog states the cost in the operator's terms and drops the
/// implementation's file names.
#[test]
fn the_clear_all_dialog_states_the_cost_without_naming_files() {
    let app = dialog_app(
        Mode::ConfirmInboxDismissAll { count: 1 },
        vec![item("#159", None, "merge_state:dirty")],
    );

    let text = body(&app);

    assert!(!text.contains("decisions.jsonl"), "{text}");
    assert!(!text.contains("ledger.json"), "{text}");
    assert!(
        text.contains("nothing on record is deleted, only removed from this list"),
        "{text}"
    );
    assert!(
        text.contains("a hidden item returns only if the same target escalates again"),
        "{text}"
    );
}

fn agent_app(mode: Mode) -> App {
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        worktree_root: temp.path().join("worktrees"),
        ..Config::default()
    };
    let mut app = App::new(
        test_support::registry_with_agent(temp.path()),
        config,
        temp.path().into(),
    );
    app.mode = mode;
    app
}

/// dropr:498 — deleting a worktree also ends its live session immediately;
/// the dialog used to say only "delete worktree?" and the agent's title.
#[test]
fn the_kill_dialog_says_it_also_ends_the_session() {
    let app = agent_app(Mode::ConfirmKill { repo: 0, agent: 0 });

    let text = body(&app);

    assert!(
        text.contains("ends the running session now and removes the worktree"),
        "{text}"
    );
}

/// A branch delete here is `git branch -D`, a force delete — unmerged work is
/// lost, not just the ref.
#[test]
fn the_delete_branch_dialog_says_the_delete_is_forced() {
    let app = agent_app(Mode::ConfirmDeleteBranch { repo: 0, agent: 0 });

    let text = body(&app);

    assert!(
        text.contains("force delete: any commits not merged elsewhere are lost"),
        "{text}"
    );
}

/// Cleanup also deletes the branch on GitHub and ends the live session, not
/// only what the old wording listed (pull main, remove worktree, delete
/// branch).
#[test]
fn the_cleanup_dialog_names_the_remote_delete_and_the_session_end() {
    let app = agent_app(Mode::ConfirmCleanup { repo: 0, agent: 0 });

    let text = body(&app);

    assert!(text.contains("here and"), "{text}");
    assert!(text.contains("on GitHub"), "{text}");
    assert!(text.contains("ends the running session"), "{text}");
}

/// Merging now runs the identical post-merge cleanup a merged-PR cleanup
/// does; the dialog used to say only that it would merge.
#[test]
fn a_merge_now_land_names_the_cleanup_that_follows() {
    let app = agent_app(Mode::ConfirmMerge {
        repo: 0,
        agent: 0,
        plan: LandPlan::MergeNow,
        head: None,
    });

    let text = body(&app);

    assert!(text.contains("It will merge now"), "{text}");
    assert!(text.contains("ends the running session"), "{text}");
}

/// A queued approval does not merge or clean up immediately, so it carries
/// none of the cleanup wording.
#[test]
fn a_queued_approval_land_does_not_claim_the_cleanup() {
    let app = agent_app(Mode::ConfirmMerge {
        repo: 0,
        agent: 0,
        plan: LandPlan::QueueApproval,
        head: None,
    });

    let text = body(&app);

    assert!(!text.contains("ends the running session"), "{text}");
}

/// Removing a Discord channel record deletes its whole history, irreversibly
/// — the dialog used to show only the channel label.
#[test]
fn the_remove_channel_dialog_says_the_history_is_gone_for_good() {
    let app = agent_app(Mode::ConfirmRemoveDiscordChannel {
        channel_id: "c1".into(),
        label: "#ops".into(),
    });

    let text = body(&app);

    assert!(
        text.contains("deletes its whole record, history included — this cannot be undone"),
        "{text}"
    );
}

/// dropr:551 — every confirmation dialog advertises `enter <verb>   esc
/// cancel`, the same shape an input dialog already used, in both locales.
#[test]
fn a_confirmation_dialog_hint_matches_the_input_dialog_shape() {
    for (locale, expected) in [
        (Locale::En, "enter delete   esc cancel"),
        (Locale::Ja, "enterで削除   escでキャンセル"),
    ] {
        let mut app = agent_app(Mode::ConfirmKill { repo: 0, agent: 0 });
        app.locale = locale;

        let text = body(&app);

        assert!(text.contains(expected), "{locale:?}: {text}");
    }
}

/// The input-dialog shape this task matched everything else to — still holds
/// after the confirmation hints changed, in both locales.
#[test]
fn an_input_dialog_hint_says_enter_and_esc() {
    for (locale, expected) in [
        (Locale::En, "enter add   esc cancel"),
        (Locale::Ja, "enterで追加   escでキャンセル"),
    ] {
        let mut app = dialog_app(
            Mode::PromptRepo {
                input: TextInput::new(),
            },
            Vec::new(),
        );
        app.locale = locale;

        let text = body(&app);

        assert!(text.contains(expected), "{locale:?}: {text}");
    }
}
