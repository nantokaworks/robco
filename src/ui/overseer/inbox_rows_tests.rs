use super::*;
use crate::{
    config::Config,
    locale::Locale,
    model::OverseerCategory,
    registry::Registry,
    ui::inbox::{InboxItem, InboxKind},
};

/// A reason far wider than any sidebar row, so the assertion that the pane
/// carries it in full is not satisfied by a lucky trim.
const LONG_REASON: &str = "merge blocked: the base branch has no required status checks \
                           and the diff touches CI configuration, which the conservative \
                           autonomy level always escalates";

fn item(kind: InboxKind, target_id: &str, session: Option<&str>) -> InboxItem {
    InboxItem {
        kind,
        target_session: session.map(ToString::to_string),
        target_id: target_id.into(),
        label: format!("{target_id} — {LONG_REASON}"),
        detail: LONG_REASON.into(),
        at: chrono::Utc::now(),
    }
}

fn inbox_app(items: Vec<InboxItem>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_inbox = items;
    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);
    app
}

fn rendered(lines: &[Line<'static>]) -> Vec<String> {
    lines.iter().map(ToString::to_string).collect()
}

#[test]
fn the_category_expands_straight_into_item_rows() {
    let app = inbox_app(vec![
        item(InboxKind::Escalation, "#159", None),
        item(InboxKind::Question, "agent-1", Some("robco-agent-1")),
    ]);
    let lines = rendered(&detail_lines(&app));

    // No count header ahead of the items: item `n` is row `n`, which is what
    // the OVERSEER frame's scroll arithmetic relies on.
    assert_eq!(lines.len(), 2);
    // `LONG_REASON` is a free-text sentence with no live session for the
    // first item, so it resolves to `Answer` and is then orphaned to
    // `Review`; the second is a `Question`, always answerable.
    assert!(lines[0].contains("[ESC] REVIEW #159"), "{lines:?}");
    assert!(lines[1].contains("[?] ANSWER agent-1"), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("inbox (")));
}

#[test]
fn the_category_numerator_equals_the_non_watch_rendered_rows() {
    // Mixed remedies: `autonomy_envelope` resolves to `Merge` regardless of
    // session, `checks_waiting` resolves to `Watch`, and the `Question` is
    // always `Answer` — two of the three are actionable.
    let mut watching = item(InboxKind::Escalation, "#1", None);
    watching.detail = "checks_waiting".into();
    let mut merge_by_hand = item(InboxKind::Escalation, "#2", None);
    merge_by_hand.detail = "autonomy_envelope".into();
    let app = inbox_app(vec![
        watching,
        merge_by_hand,
        item(InboxKind::Question, "agent-1", Some("robco-agent-1")),
    ]);

    let (summary, _) = crate::ui::overseer::category_summary(&app, OverseerCategory::Inbox);
    assert_eq!(summary, "2/3 actionable", "{summary}");

    let non_watch_rows = rendered(&detail_lines(&app))
        .iter()
        .filter(|line| !line.contains("WATCH"))
        .count();
    assert_eq!(non_watch_rows, 2);
}

#[test]
fn an_empty_inbox_renders_its_empty_state() {
    assert_eq!(rendered(&detail_lines(&inbox_app(Vec::new()))), ["none"]);
}

#[test]
fn the_preview_describes_the_selected_item_and_not_the_other_items() {
    let app = inbox_app(vec![
        item(InboxKind::Escalation, "#159", Some("robco-agent-1")),
        item(InboxKind::Question, "agent-2", Some("robco-agent-2")),
    ]);

    let (title, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    assert_eq!(title, "OVERSEER / Inbox / #159");
    assert!(body.contains("kind: escalation"), "{body}");
    assert!(body.contains("target: #159"), "{body}");
    assert!(body.contains("session: robco-agent-1"), "{body}");
    // The whole reason, not the row label the sidebar trims to its width.
    assert!(body.contains(LONG_REASON), "{body}");
    // The other item is already on screen in the left frame; repeating it here
    // is what this pane used to do and says nothing about the selected row.
    assert!(!body.contains("agent-2"), "{body}");
}

#[test]
fn a_display_only_item_says_why_it_cannot_be_answered() {
    let app = inbox_app(vec![item(InboxKind::Escalation, "#159", None)]);

    let (_, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    assert!(
        body.contains("display-only — no live session to answer or approve"),
        "{body}"
    );
}

#[test]
fn a_japanese_locale_localizes_the_remedy_means_and_next_sentences() {
    let mut app = inbox_app(vec![item(InboxKind::Question, "agent-1", Some("robco-1"))]);
    app.locale = Locale::Ja;

    let (_, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    // WORKER_QUESTION's means / next — see `locale::ja::remedy`.
    assert!(
        body.contains("workerは自分のセッション内で確認プロンプトを待っています"),
        "{body}"
    );
    assert!(
        body.contains("Enterを押して回答を入力してください"),
        "{body}"
    );
    // Labels stay English even in the Japanese locale (dropr:377).
    assert!(body.contains("what this means"), "{body}");
    assert!(body.contains("next step"), "{body}");
}

#[test]
fn a_known_reason_code_gets_a_localized_sentence_ahead_of_the_verbatim_code() {
    let mut escalation = item(InboxKind::Escalation, "#42", None);
    escalation.detail = "merge_state:dirty".into();
    let mut app = inbox_app(vec![escalation]);
    app.locale = Locale::Ja;

    let (_, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    // `humanize::EXACT`'s sentence for "merge_state:dirty", localized —
    // see `locale::ja::humanize`.
    assert!(body.contains("ブランチがbaseと競合しています。"), "{body}");
    // The raw code always stays, verbatim and untranslated.
    assert!(body.contains("merge_state:dirty"), "{body}");
}

#[test]
fn an_unknown_reason_code_renders_verbatim_with_no_extra_sentence_line() {
    let app = inbox_app(vec![item(InboxKind::Escalation, "#159", None)]);

    let (_, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    assert!(body.contains(LONG_REASON), "{body}");
    // No humanized line was inserted: the reason section is exactly the raw
    // detail, one line per line of `LONG_REASON`.
    let reason_index = rendered(&text.lines)
        .iter()
        .position(|line| line == "reason")
        .expect("reason label");
    assert_eq!(
        rendered(&text.lines)[reason_index + 1],
        LONG_REASON.to_string()
    );
}

#[test]
fn a_stale_index_previews_a_notice_rather_than_panicking() {
    // The list re-aggregates under the cursor, so an index can outlive its row
    // between the key press that moved the cursor and the next draw.
    let app = inbox_app(Vec::new());

    let (title, text) = item_preview(&app, 3);

    assert_eq!(title, "OVERSEER / Inbox");
    assert_eq!(rendered(&text.lines), ["item is no longer listed"]);
}
