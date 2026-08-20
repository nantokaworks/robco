//! Showing a URL in the TUI as an OSC 8 hyperlink (dropr:512).
//!
//! `browser.rs` hands the operator a dropr task URL. Over SSH it cannot open
//! a browser, so the URL is copied with OSC 52 and also shown as plain text
//! in the footer message. This module marks that plain text up so a terminal
//! with OSC 8 support (iTerm2, WezTerm, Kitty, Ghostty, Windows Terminal)
//! makes it clickable.
//!
//! **Why this is not a widget.** OSC 8 marks up text *in the terminal grid*,
//! and the grid belongs to ratatui. ratatui 0.29 has no hyperlink support,
//! and the two obvious workarounds do not hold:
//!
//! - Putting the escape inside a [`Cell`] symbol does reach the terminal,
//!   because the backend prints the symbol as-is. But [`Buffer::diff`] then
//!   measures that symbol with `unicode_width`, which counts the escape bytes
//!   as normal columns. One cell would report a width of about eighty, and
//!   the diff would skip the eighty cells after it, so the rest of the row
//!   would show stale text.
//! - Writing the escape to the tty before the frame would put every cell of
//!   that frame inside the link, not just the URL.
//!
//! **What this does instead.** After ratatui finishes a frame, the URL is
//! already painted at some cell. This module finds those cells, then repaints
//! exactly them with the OSC 8 markup wrapped around the same characters and
//! the same colours. ratatui does not know the markup is there, but it does
//! not have to: the markup draws nothing, so ratatui's own idea of the frame
//! stays true. The whole link — opener, text, closer — goes out in one write,
//! so no repaint can split it and leave the terminal stuck inside an open
//! link.
//!
//! A cell keeps its link until something repaints it, and ratatui's diff
//! leaves a cell alone when its character did not change. A later frame could
//! therefore reuse one of these cells for the same character and keep the old
//! link on it. `event_loop` closes that gap: when the link moves or goes
//! away, it asks for a full redraw, which repaints every cell.
//!
//! A terminal without OSC 8 support drops the sequence the way it drops any
//! unknown OSC, so the line still reads as plain text with no stray bytes.
//! tmux keeps hyperlinks from version 3.4 on and strips them before that.
//!
//! robco captures the mouse, so in most terminals the click needs the usual
//! modifier (⌘-click, or Ctrl-click on Linux and Windows) to reach the
//! terminal instead of the app.
//!
//! [`Cell`]: ratatui::buffer::Cell
//! [`Buffer::diff`]: ratatui::buffer::Buffer::diff

use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{
        Attribute as TermAttribute, Color as TermColor, Print, SetAttribute, SetBackgroundColor,
        SetForegroundColor, SetUnderlineColor,
    },
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
};

/// The only scheme robco links. A task console URL is always `https`, and
/// keeping the set this small means a message can never turn something like a
/// `file:` path into a link by accident.
const SCHEME: &str = "https://";

/// Ends an OSC 8 link. The empty slot where the URL would go is what tells
/// the terminal the link stops here.
const CLOSE: &str = "\x1b]8;;\x1b\\";

/// A URL the finished frame painted, and where it landed.
#[derive(PartialEq, Eq)]
pub(super) struct Painted {
    url: String,
    x: u16,
    y: u16,
    fg: TermColor,
    bg: TermColor,
    underline: TermColor,
    modifier: Modifier,
}

/// Picks the URL out of a status message, if it has one.
///
/// The URL runs to the first space, which is why [`Painted`] can trust it to
/// be one unbroken run of cells. Anything with a control byte or a non-ASCII
/// byte is refused: such a URL could carry its own escape sequence, and the
/// cell search below assumes one byte per column.
pub(super) fn url_in(message: &str) -> Option<&str> {
    let start = message.find(SCHEME)?;
    let url = message[start..].split_whitespace().next()?;
    let printable = url.bytes().all(|byte| byte.is_ascii_graphic());
    (url.len() > SCHEME.len() && printable).then_some(url)
}

/// Finds `url` inside `area` of the finished frame.
///
/// Reading the cells back is what keeps this honest: it does not have to know
/// how the footer centres or truncates its line. A URL the frame cut short is
/// simply not found, and a cut-short URL is one robco should not link anyway.
pub(super) fn find(buffer: &Buffer, area: Rect, url: &str) -> Option<Painted> {
    let bytes = url.as_bytes();
    let len = u16::try_from(bytes.len()).ok()?;
    if len == 0 || len > area.width {
        return None;
    }
    for y in area.top()..area.bottom() {
        for x in area.left()..=(area.right() - len) {
            if !starts_at(buffer, x, y, bytes) {
                continue;
            }
            let cell = buffer.cell((x, y))?;
            return Some(Painted {
                url: url.to_string(),
                x,
                y,
                fg: term_color(cell.fg),
                bg: term_color(cell.bg),
                underline: term_color(cell.underline_color),
                modifier: cell.modifier,
            });
        }
    }
    None
}

fn starts_at(buffer: &Buffer, x: u16, y: u16, bytes: &[u8]) -> bool {
    bytes.iter().enumerate().all(|(offset, byte)| {
        u16::try_from(offset)
            .ok()
            .and_then(|offset| buffer.cell((x + offset, y)))
            .is_some_and(|cell| cell.symbol().as_bytes() == [*byte])
    })
}

/// Repaints the URL cells with the OSC 8 markup around them, then puts the
/// cursor back where the frame left it.
///
/// The colours come from the cells themselves, so the URL looks exactly as it
/// did a moment ago. Everything is reset afterwards, which is the state
/// ratatui leaves the terminal in at the end of its own draw.
pub(super) fn draw(link: &Painted, restore_to: (u16, u16)) -> io::Result<()> {
    let mut out = io::stdout();
    write(&mut out, link, restore_to)?;
    out.flush()
}

fn write<W: Write>(out: &mut W, link: &Painted, restore_to: (u16, u16)) -> io::Result<()> {
    queue!(
        out,
        MoveTo(link.x, link.y),
        SetAttribute(TermAttribute::Reset),
        SetForegroundColor(link.fg),
        SetBackgroundColor(link.bg),
        SetUnderlineColor(link.underline),
    )?;
    for attribute in attributes(link.modifier) {
        queue!(out, SetAttribute(attribute))?;
    }
    queue!(
        out,
        Print(format!("\x1b]8;;{}\x1b\\", link.url)),
        Print(&link.url),
        Print(CLOSE),
        SetAttribute(TermAttribute::Reset),
        SetForegroundColor(TermColor::Reset),
        SetBackgroundColor(TermColor::Reset),
        SetUnderlineColor(TermColor::Reset),
        MoveTo(restore_to.0, restore_to.1),
    )
}

/// ratatui's colours in crossterm's words.
///
/// ratatui ships this mapping already, but its own copy targets the crossterm
/// version ratatui pins (0.28), while robco writes with crossterm 0.29. The
/// two are different types to the compiler, so the mapping is repeated here.
/// It follows ratatui's: a ratatui "normal" colour is a crossterm "dark" one,
/// and a ratatui "light" colour is the plain crossterm name.
fn term_color(color: Color) -> TermColor {
    match color {
        Color::Reset => TermColor::Reset,
        Color::Black => TermColor::Black,
        Color::Red => TermColor::DarkRed,
        Color::Green => TermColor::DarkGreen,
        Color::Yellow => TermColor::DarkYellow,
        Color::Blue => TermColor::DarkBlue,
        Color::Magenta => TermColor::DarkMagenta,
        Color::Cyan => TermColor::DarkCyan,
        Color::Gray => TermColor::Grey,
        Color::DarkGray => TermColor::DarkGrey,
        Color::LightRed => TermColor::Red,
        Color::LightGreen => TermColor::Green,
        Color::LightYellow => TermColor::Yellow,
        Color::LightBlue => TermColor::Blue,
        Color::LightMagenta => TermColor::Magenta,
        Color::LightCyan => TermColor::Cyan,
        Color::White => TermColor::White,
        Color::Indexed(index) => TermColor::AnsiValue(index),
        Color::Rgb(r, g, b) => TermColor::Rgb { r, g, b },
    }
}

/// The crossterm attributes that turn `modifier` on from a reset state. This
/// is the "added" half of what ratatui's own crossterm backend queues; the
/// "removed" half is not needed, because the reset above already cleared
/// everything.
fn attributes(modifier: Modifier) -> Vec<TermAttribute> {
    const TABLE: [(Modifier, TermAttribute); 8] = [
        (Modifier::REVERSED, TermAttribute::Reverse),
        (Modifier::BOLD, TermAttribute::Bold),
        (Modifier::ITALIC, TermAttribute::Italic),
        (Modifier::UNDERLINED, TermAttribute::Underlined),
        (Modifier::DIM, TermAttribute::Dim),
        (Modifier::CROSSED_OUT, TermAttribute::CrossedOut),
        (Modifier::SLOW_BLINK, TermAttribute::SlowBlink),
        (Modifier::RAPID_BLINK, TermAttribute::RapidBlink),
    ];

    TABLE
        .iter()
        .filter(|(flag, _)| modifier.contains(*flag))
        .map(|(_, attribute)| *attribute)
        .collect()
}

#[cfg(test)]
#[path = "hyperlink_tests.rs"]
mod tests;
