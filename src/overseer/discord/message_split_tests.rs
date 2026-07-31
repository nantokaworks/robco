use super::*;

#[test]
fn a_short_reply_stays_one_untouched_message() {
    let text = "all quiet\n- worker w1 idle";
    assert_eq!(split_message(text, 1900), vec![text.to_string()]);
}

#[test]
fn a_long_reply_splits_at_a_line_boundary() {
    assert_eq!(
        split_message("aaa\nbbb\nccc", 10),
        vec!["aaa\nbbb".to_string(), "ccc".to_string()]
    );
}

#[test]
fn a_split_inside_a_code_fence_closes_and_reopens_it() {
    let chunks = split_message("```rust\ncode\nmore\n```\ndone", 16);
    assert_eq!(
        chunks,
        vec![
            "```rust\ncode\n```".to_string(),
            "```rust\nmore\n```".to_string(),
            "done".to_string(),
        ]
    );
    for chunk in &chunks {
        assert!(chunk.chars().count() <= 16, "{chunk}");
        assert_eq!(chunk.matches("```").count() % 2, 0, "{chunk}");
    }
}

#[test]
fn a_trailing_unclosed_fence_is_closed() {
    assert_eq!(split_message("```\nx", 50), vec!["```\nx\n```".to_string()]);
}

#[test]
fn an_inline_fence_pair_does_not_toggle_the_state() {
    assert_eq!(
        split_message("a ```b``` c\nnext line!", 12),
        vec!["a ```b``` c".to_string(), "next line!".to_string()]
    );
}

#[test]
fn a_single_oversized_line_still_hard_cuts() {
    assert_eq!(
        split_message("abcdefghijklmno", 10),
        vec!["abcdefghij".to_string(), "klmno".to_string()]
    );
}

#[test]
fn an_oversized_line_inside_a_fence_keeps_every_message_well_formed() {
    let text = format!("```\n{}\n```", "x".repeat(30));
    for chunk in split_message(&text, 16) {
        assert!(chunk.chars().count() <= 16, "{chunk}");
        assert_eq!(chunk.matches("```").count() % 2, 0, "{chunk}");
    }
}

#[test]
fn every_message_of_a_real_sized_reply_fits_the_discord_cap() {
    let paragraph = "The worker finished the build and the tests passed. ".repeat(10);
    let text = format!(
        "{paragraph}\n\n```text\n{}\n```\n\n{paragraph}",
        "log line with some detail\n".repeat(60)
    );
    let chunks = split_message(&text, MESSAGE_LIMIT);
    assert!(chunks.len() > 1);
    for chunk in &chunks {
        assert!(chunk.chars().count() <= MESSAGE_LIMIT, "{}", chunk.len());
        assert_eq!(chunk.matches("```").count() % 2, 0, "{chunk}");
    }
}

#[test]
fn empty_text_stays_a_single_empty_message() {
    assert_eq!(split_message("", 10), vec![String::new()]);
}
