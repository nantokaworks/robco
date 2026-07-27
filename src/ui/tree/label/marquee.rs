//! A title that doesn't fit its available width either scrolls (a selected
//! row marquees to reveal the rest over time) or truncates with an ellipsis
//! (an unselected row stays put).

use std::time::Duration;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::prefix_within;

const START_PAUSE: Duration = Duration::from_millis(1_000);
const STEP: Duration = Duration::from_millis(300);
const END_PAUSE: Duration = Duration::from_millis(1_000);

pub(super) fn display(title: &str, available: usize, selected: bool, elapsed: Duration) -> String {
    if UnicodeWidthStr::width(title) <= available {
        title.to_string()
    } else if selected {
        marquee(title, available, elapsed)
    } else {
        truncate(title, available)
    }
}

fn truncate(title: &str, available: usize) -> String {
    if UnicodeWidthStr::width(title) <= available {
        return title.to_string();
    }
    if available == 0 {
        return String::new();
    }

    let content_width = available - 1;
    let mut result = prefix_within(title, content_width).to_string();
    result.push('…');
    result
}

fn marquee(title: &str, available: usize, elapsed: Duration) -> String {
    if available == 0 {
        return String::new();
    }
    let offset = marquee_offset(UnicodeWidthStr::width(title), available, elapsed);
    let start = byte_at_or_after_width(title, offset);
    prefix_within(&title[start..], available).to_string()
}

fn marquee_offset(title_width: usize, available: usize, elapsed: Duration) -> usize {
    let max_offset = title_width.saturating_sub(available);
    if max_offset == 0 {
        return 0;
    }
    let travel = STEP * u32::try_from(max_offset).unwrap_or(u32::MAX);
    let cycle = START_PAUSE + travel + END_PAUSE;
    let position = elapsed.as_millis() % cycle.as_millis();
    let start_ms = START_PAUSE.as_millis();
    if position <= start_ms {
        0
    } else if position >= (START_PAUSE + travel).as_millis() {
        max_offset
    } else {
        usize::try_from((position - start_ms) / STEP.as_millis())
            .unwrap_or(max_offset)
            .min(max_offset)
    }
}

fn byte_at_or_after_width(value: &str, target: usize) -> usize {
    let mut width = 0;
    for (index, character) in value.char_indices() {
        if width >= target {
            return index;
        }
        width += UnicodeWidthChar::width(character).unwrap_or(0);
    }
    value.len()
}

#[cfg(test)]
#[path = "marquee_tests.rs"]
mod tests;
