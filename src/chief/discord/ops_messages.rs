use crate::chief::triage::ExceptionCase;
use std::path::Path;

pub(super) fn opening_message(case: &ExceptionCase, capture: &str) -> String {
    format!(
        "**Escalation `{}`**\nQuestion: {}\nRepo: `{}`\nTask: `{}` ({})\nWorker: `{}`\nTask state: `{}`\nRecent capture:\n```text\n{}\n```",
        case.id,
        case.reason,
        case.repo,
        case.task_id,
        case.display_id,
        case.worker_id,
        case.task_state,
        capture.replace("```", "` ` `")
    )
}

pub(super) fn capture_summary(root: &Path, case_id: &str) -> String {
    let Ok(briefing) = std::fs::read_to_string(root.join(case_id).join("briefing.md")) else {
        return "unavailable".into();
    };
    let capture = briefing
        .split("<<<EXTERNAL_DATA RECENT_TMUX_CAPTURE>>>\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<<<END_EXTERNAL_DATA>>>").next())
        .unwrap_or("unavailable");
    let lines: Vec<_> = capture
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let start = lines.len().saturating_sub(6);
    lines[start..].join("\n").chars().take(500).collect()
}
