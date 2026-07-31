//! Row-based rendering for Discord command responses: joins rows into a
//! plain message or a code block while staying under the Discord message
//! length limit, so truncation is always an explicit `… and N more` tail.

/// Discord plain messages cap at 2000 chars and `send_text` splits longer
/// text into more messages at 1900. Staying under that here keeps a command
/// response a single message: dropped rows become a `… and N more` tail
/// instead of a follow-up message.
const RESPONSE_BUDGET: usize = 1800;

pub(super) fn bounded_rows(rows: &[String]) -> String {
    let mut used = 0;
    let mut kept = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if used + row.chars().count() + 1 > RESPONSE_BUDGET {
            kept.push(format!("… and {} more", rows.len() - index));
            break;
        }
        used += row.chars().count() + 1;
        kept.push(row.clone());
    }
    kept.join("\n")
}

pub(super) fn code_block(rows: &[String]) -> String {
    format!("```text\n{}\n```", bounded_rows(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflowing_rows_collapse_into_a_more_tail() {
        let rows: Vec<_> = (0..100)
            .map(|index| format!("row-{index:03} {}", "x".repeat(40)))
            .collect();
        let bounded = bounded_rows(&rows);
        assert!(bounded.chars().count() <= RESPONSE_BUDGET + 20);
        assert!(bounded.lines().last().unwrap().starts_with("… and "));
        assert!(bounded.lines().last().unwrap().ends_with(" more"));
    }

    #[test]
    fn short_rows_render_without_a_tail() {
        let rows = vec!["a".to_string(), "b".to_string()];
        assert_eq!(bounded_rows(&rows), "a\nb");
        assert_eq!(code_block(&rows), "```text\na\nb\n```");
    }
}
