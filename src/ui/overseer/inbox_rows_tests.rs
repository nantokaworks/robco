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
                           and the diff touches CI configuration, which no reviewer has \
                           looked at yet";

fn item(kind: InboxKind, target_id: &str, session: Option<&str>) -> InboxItem {
    InboxItem {
        kind,
        repo: None,
        target_session: session.map(ToString::to_string),
        target_id: target_id.into(),
        label: format!("{target_id} — {LONG_REASON}"),
        detail: LONG_REASON.into(),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
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

/// Renders `lines` into a buffer exactly `width` columns wide, the way the
/// OVERSEER frame's `Paragraph` does — so a row that overflows is clipped by
/// the widget itself, the same clip an operator's real terminal applies.
fn rendered_at_width(lines: &[Line<'static>], width: u16) -> Vec<String> {
    let height = u16::try_from(lines.len()).unwrap();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(
                ratatui::widgets::Paragraph::new(lines.to_vec()),
                frame.area(),
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer.cell((x, y)).unwrap().symbol())
                .collect()
        })
        .collect()
}

#[test]
fn the_category_expands_straight_into_item_rows() {
    let mut answerable = item(InboxKind::Escalation, "agent-1", Some("robco-agent-1"));
    answerable.detail = "checks_not_green".into();
    let app = inbox_app(vec![item(InboxKind::Escalation, "#159", None), answerable]);
    let lines = rendered(&detail_lines(&app));

    // No count header ahead of the items: item `n` is row `n`, which is what
    // the OVERSEER frame's scroll arithmetic relies on.
    assert_eq!(lines.len(), 2);
    // `LONG_REASON` is a free-text sentence with no live session for the
    // first item, so it resolves to `Answer` and is then orphaned to
    // `Review`; the second has a live session and a table-known reason, so
    // it stays `Answer`.
    assert!(lines[0].contains("[ESC] REVIEW #159"), "{lines:?}");
    assert!(lines[1].contains("[ESC] ANSWER agent-1"), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("inbox (")));
}

#[test]
fn the_category_numerator_equals_the_non_watch_rendered_rows() {
    // Mixed remedies: `merge_request_stale` resolves to `Merge` regardless of
    // session, `checks_waiting` resolves to `Watch`, and a table-known reason
    // with a live session stays `Answer` — two of the three are actionable.
    let mut watching = item(InboxKind::Escalation, "#1", None);
    watching.detail = "checks_waiting".into();
    let mut merge_by_hand = item(InboxKind::Escalation, "#2", None);
    merge_by_hand.detail = "merge_request_stale".into();
    let mut answerable = item(InboxKind::Escalation, "agent-1", Some("robco-agent-1"));
    answerable.detail = "checks_not_green".into();
    let app = inbox_app(vec![watching, merge_by_hand, answerable]);

    let (summary, _) = crate::ui::overseer::category_summary(&app, OverseerCategory::Inbox);
    assert_eq!(summary, "2/3 actionable", "{summary}");

    let non_watch_rows = rendered(&detail_lines(&app))
        .iter()
        .filter(|line| !line.contains("WATCH"))
        .count();
    assert_eq!(non_watch_rows, 2);
}

#[test]
fn a_row_names_its_repository() {
    let mut row = item(InboxKind::Escalation, "#159", None);
    row.repo = Some("robco".into());
    let app = inbox_app(vec![row]);
    let lines = rendered(&detail_lines(&app));

    assert!(lines[0].contains("[ESC] REVIEW robco #159"), "{lines:?}");
}

/// dropr:498 — a row says what happened, not only its kind and target.
#[test]
fn a_row_with_a_known_reason_shows_the_humanized_sentence() {
    let mut row = item(InboxKind::Escalation, "#159", None);
    row.detail = "merge_state:dirty".into();
    let app = inbox_app(vec![row]);
    let lines = rendered(&detail_lines(&app));

    assert!(
        lines[0].contains("[ESC] REVIEW #159 — The branch has conflicts with its base."),
        "{lines:?}"
    );
}

/// An unrecognised reason still says something rather than nothing: the
/// row falls back to the reason's own first line, verbatim.
#[test]
fn a_row_with_an_unknown_reason_shows_the_raw_reason() {
    let app = inbox_app(vec![item(InboxKind::Escalation, "#159", None)]);
    let lines = rendered(&detail_lines(&app));

    assert!(
        lines[0].contains(&format!("[ESC] REVIEW #159 — {LONG_REASON}")),
        "{lines:?}"
    );
}

/// The reason is the row's lowest-priority field: it renders after the
/// target id and gives way to width before the target id does, so the row
/// still reads at the narrowest sidebar even when a reason cannot fit.
#[test]
fn at_the_narrowest_sidebar_width_the_reason_gives_way_before_the_target_id() {
    let mut row = item(InboxKind::Escalation, "#159", None);
    row.repo = Some("robco".into());
    row.detail = "merge_state:dirty".into();
    let app = inbox_app(vec![row]);
    let lines = detail_lines(&app);

    let narrowest = rendered_at_width(&lines, 24);
    assert!(narrowest[0].contains("[ESC] REVIEW robco"), "{narrowest:?}");
    assert!(
        !narrowest[0].contains("conflicts"),
        "reason showed at the width the target id itself barely fits: {narrowest:?}"
    );

    // Comfortably wide: the reason shows in full.
    let wide = rendered_at_width(&lines, 80);
    assert!(
        wide[0].contains("The branch has conflicts with its base."),
        "{wide:?}"
    );
}

#[test]
fn a_row_with_no_matching_ledger_entry_still_renders() {
    // `repo` stays `None` for a decision-sourced row whose ledger lookup
    // came up empty — the row renders exactly as it did before repositories
    // were tracked, rather than a blank gap where the name would go.
    let row = item(InboxKind::Escalation, "#159", None);
    let app = inbox_app(vec![row]);
    let lines = rendered(&detail_lines(&app));

    assert!(lines[0].contains("[ESC] REVIEW #159"), "{lines:?}");
}

/// dropr:478 — with four repositories registered, `#296` alone identifies
/// nothing; the row's own tag and repository are what tell an operator
/// whether it is theirs. At the sidebar's narrowest width (24 columns) the
/// target id is what gives way first.
#[test]
fn at_the_narrowest_sidebar_width_the_target_id_gives_way_first() {
    let mut row = item(InboxKind::Escalation, "#159", None);
    row.repo = Some("robco".into());
    let app = inbox_app(vec![row]);
    let lines = detail_lines(&app);

    // At the sidebar's narrowest width the move tag and the repository
    // still read in full — they are what tell an operator whether the row
    // is theirs.
    let narrowest = rendered_at_width(&lines, 24);
    assert!(narrowest[0].contains("[ESC] REVIEW robco"), "{narrowest:?}");

    // A width too tight even for that drops the target id first, never the
    // tag or the repository.
    let narrower = rendered_at_width(&lines, 20);
    assert!(narrower[0].contains("REVIEW robco"), "{narrower:?}");
    assert!(!narrower[0].contains("#159"), "{narrower:?}");
}

#[test]
fn the_preview_shows_the_repository_spelled_out() {
    let mut row = item(InboxKind::Escalation, "#159", None);
    row.repo = Some("robco".into());
    let app = inbox_app(vec![row]);

    let (_, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    assert!(body.contains("repo: robco"), "{body}");
}

#[test]
fn the_preview_omits_repo_when_the_row_has_none() {
    let app = inbox_app(vec![item(InboxKind::Escalation, "#159", None)]);

    let (_, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    assert!(!body.contains("repo:"), "{body}");
}

#[test]
fn an_empty_inbox_renders_its_empty_state() {
    assert_eq!(rendered(&detail_lines(&inbox_app(Vec::new()))), ["none"]);
}

#[test]
fn the_preview_describes_the_selected_item_and_not_the_other_items() {
    let app = inbox_app(vec![
        item(InboxKind::Escalation, "#159", Some("robco-agent-1")),
        item(InboxKind::Escalation, "agent-2", Some("robco-agent-2")),
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

/// dropr:461 — a row with a matching ledger entry's facts shows its number,
/// title, size, and failing check.
#[test]
fn a_row_with_pr_facts_shows_its_own_case() {
    let mut row = item(InboxKind::Escalation, "#159", None);
    row.pr_url = Some("https://github.com/nantokaworks/robco/pull/42".into());
    row.pr_facts = Some(crate::overseer::ledger::PrFacts {
        title: "Fix the thing".into(),
        files_changed: 4,
        lines_changed: 42,
        failed_checks: vec!["validate / Validate".into()],
    });
    let app = inbox_app(vec![row]);

    let (_, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    assert!(body.contains("pull request: #42"), "{body}");
    assert!(body.contains("title: Fix the thing"), "{body}");
    assert!(body.contains("size: 4 files, 42 lines"), "{body}");
    assert!(body.contains("failed check: validate / Validate"), "{body}");
}

#[test]
fn a_row_with_no_pr_facts_renders_without_them() {
    let app = inbox_app(vec![item(InboxKind::Escalation, "#159", None)]);

    let (_, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    assert!(!body.contains("pull request:"), "{body}");
    assert!(!body.contains("size:"), "{body}");
    assert!(!body.contains("failed check:"), "{body}");
}

#[test]
fn a_japanese_locale_localizes_the_remedy_means_and_next_sentences() {
    // An `Answer` move with no live session downgrades to `ORPHANED`, a
    // fixed remedy independent of the reason text — see `locale::ja::remedy`.
    let mut orphaned = item(InboxKind::Escalation, "agent-1", None);
    orphaned.detail = "merge_state:dirty".into();
    let mut app = inbox_app(vec![orphaned]);
    app.locale = Locale::Ja;

    let (_, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    assert!(
        body.contains("回答が必要でしたが、workerのセッションは既に終了しています"),
        "{body}"
    );
    assert!(body.contains("修正を送るセッションがありません"), "{body}");
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
