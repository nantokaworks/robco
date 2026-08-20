use ratatui::{
    Terminal,
    backend::TestBackend,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::*;
use crate::{
    config::Config,
    registry::Registry,
    ui::{App, layout, theme::DEFAULT as THEME, tree},
};

const URL: &str = "https://dropr.sh/nantokaworks/robco/tasks/t3PDYwtmcmJeclnZQVbRl";

/// Paints `text` into a one-row buffer `width` columns wide, the way the
/// footer paints its message: one styled span, centred, no wrapping.
fn painted(text: &str, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, 1);
    let mut buffer = Buffer::empty(area);
    Paragraph::new(Line::from(Span::styled(
        text.to_string(),
        THEME.hint_style(),
    )))
    .alignment(ratatui::layout::Alignment::Center)
    .render(area, &mut buffer);
    buffer
}

#[test]
fn reads_the_url_out_of_the_clipboard_message() {
    let message = format!("copied the URL for #512: {URL}");
    assert_eq!(url_in(&message), Some(URL));
}

#[test]
fn stops_the_url_at_the_first_space() {
    let message = format!("{URL} was copied");
    assert_eq!(url_in(&message), Some(URL));
}

#[test]
fn a_message_without_a_url_has_nothing_to_link() {
    assert_eq!(url_in("task is no longer listed"), None);
    assert_eq!(url_in("could not open #512: xdg-open failed"), None);
}

#[test]
fn refuses_a_url_that_carries_its_own_escape() {
    let message = "copied: https://example.com/\x1b]8;;evil\x1b\\ end";
    let url = url_in(message).unwrap_or_default();
    assert!(
        !url.contains('\x1b'),
        "an escape byte must never reach the OSC 8 payload: {url:?}"
    );
}

#[test]
fn finds_the_url_the_frame_painted() {
    let buffer = painted(&format!("copied the URL for #512: {URL}"), 100);
    let link = find(&buffer, buffer.area, URL).expect("the full URL is on screen");
    assert_eq!(link.url, URL);
    assert_eq!(link.y, 0);
    assert_eq!(link.fg, term_color(THEME.hint));
    // The cells the search reports really do hold the URL.
    let painted: String = (0..URL.len() as u16)
        .filter_map(|offset| buffer.cell((link.x + offset, link.y)))
        .map(|cell| cell.symbol().to_string())
        .collect();
    assert_eq!(painted, URL);
}

#[test]
fn a_truncated_url_is_not_linked() {
    let buffer = painted(&format!("copied the URL for #512: {URL}"), 40);
    assert!(
        find(&buffer, buffer.area, URL).is_none(),
        "half a URL must not become a whole link"
    );
}

#[test]
fn a_url_outside_the_searched_area_is_not_linked() {
    let buffer = painted(URL, 100);
    let elsewhere = Rect::new(0, 0, 10, 1);
    assert!(find(&buffer, elsewhere, URL).is_none());
}

#[test]
fn wraps_the_url_in_osc_8_and_puts_the_cursor_back() {
    let buffer = painted(URL, 100);
    let link = find(&buffer, buffer.area, URL).expect("the full URL is on screen");
    let mut out = Vec::new();
    write(&mut out, &link, (3, 24)).expect("a Vec never fails to write");
    let payload = String::from_utf8(out).expect("crossterm writes UTF-8");

    let opener = format!("\x1b]8;;{URL}\x1b\\");
    assert!(payload.contains(&format!("{opener}{URL}{CLOSE}")));
    // One write carries the whole link, so no repaint can split it.
    assert_eq!(payload.matches(&opener).count(), 1);
    assert_eq!(payload.matches(CLOSE).count(), 1);
    // The frame's cursor is restored last, after the colours are reset.
    assert!(payload.ends_with("\x1b[25;4H"));
}

#[test]
fn repaints_the_url_in_the_colour_the_frame_used() {
    let mut buffer = painted(URL, 100);
    let area = buffer.area;
    Paragraph::new(Line::from(Span::styled(
        URL,
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )))
    .render(area, &mut buffer);
    let link = find(&buffer, area, URL).expect("the full URL is on screen");
    let mut out = Vec::new();
    write(&mut out, &link, (0, 0)).expect("a Vec never fails to write");
    let payload = String::from_utf8(out).expect("crossterm writes UTF-8");

    assert!(
        payload.contains("\x1b[38;5;1m"),
        "red foreground: {payload:?}"
    );
    assert!(payload.contains("\x1b[1m"), "bold: {payload:?}");
}

#[test]
fn a_plain_style_needs_no_attributes() {
    assert!(attributes(THEME.hint_style().add_modifier).is_empty());
}

/// The one test that pins the wiring: the real footer, drawn by the real
/// widget, must put the URL inside the zone `event_loop` searches.
#[test]
fn the_real_footer_puts_the_url_where_the_search_looks() {
    let temp = tempfile::tempdir().expect("a temp dir");
    let app = App::new(Registry::default(), Config::default(), temp.path().into());
    let message = format!("copied the URL for #512: {URL}");
    let visible = app.visible();
    let mut terminal = Terminal::new(TestBackend::new(120, 20)).expect("a test terminal");
    terminal
        .draw(|frame| tree::draw(frame, &app, &visible, Some(&message)))
        .expect("the frame draws");

    let buffer = terminal.backend().buffer();
    let hints = layout::footer(layout::root(buffer.area).footer).zones.hints;
    let url = url_in(&message).expect("the message carries a URL");
    let link = find(buffer, hints, url).expect("the footer painted the whole URL");
    assert_eq!(link.url, URL);
}

/// A terminal too narrow for the whole URL truncates it, and half a URL must
/// not become a link.
#[test]
fn a_narrow_terminal_gets_no_link() {
    let temp = tempfile::tempdir().expect("a temp dir");
    let app = App::new(Registry::default(), Config::default(), temp.path().into());
    let message = format!("copied the URL for #512: {URL}");
    let visible = app.visible();
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).expect("a test terminal");
    terminal
        .draw(|frame| tree::draw(frame, &app, &visible, Some(&message)))
        .expect("the frame draws");

    let buffer = terminal.backend().buffer();
    let hints = layout::footer(layout::root(buffer.area).footer).zones.hints;
    let url = url_in(&message).expect("the message carries a URL");
    assert!(find(buffer, hints, url).is_none());
}
