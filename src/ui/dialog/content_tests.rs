use ratatui::layout::Rect;

use super::*;
use crate::{
    config::Config,
    locale::Locale,
    registry::Registry,
    ui::{LandPlan, test_support, text_input::TextInput},
};

fn dialog_app(mode: Mode) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
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
        let mut app = dialog_app(Mode::PromptRepo {
            input: TextInput::new(),
        });
        app.locale = locale;

        let text = body(&app);

        assert!(text.contains(expected), "{locale:?}: {text}");
    }
}
