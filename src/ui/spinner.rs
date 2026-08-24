use std::time::Duration;

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// A visually distinct spinner for the companion TERM (shell) session, so a
/// running shell command reads differently from the AI's own `run` spinner.
const TERM_FRAMES: [&str; 8] = ["▖", "▘", "▝", "▗", "▚", "▞", "▙", "▟"];
const MCP_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
/// robco's own working spinner (dropr:545), for work robco runs itself
/// rather than work an agent runs. It keeps the arrow reading of the old
/// static `⇄` merge glyph so the meaning does not change, and adds the
/// motion that glyph never had. Its own frame set, so robco's work never
/// looks like an agent's `run` spinner, the TERM shell spinner, or the MCP
/// spinner.
const ROBCO_FRAMES: [&str; 4] = ["⇠", "⇡", "⇢", "⇣"];
const FRAME_INTERVAL_MS: u128 = 120;
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(FRAME_INTERVAL_MS as u64);

/// Pick the animation frame from elapsed wall-clock time, so the spinner runs
/// at a steady rate regardless of how often the UI is redrawn.
pub(crate) fn frame(elapsed: Duration) -> &'static str {
    let idx = (elapsed.as_millis() / FRAME_INTERVAL_MS) as usize;
    FRAMES[idx % FRAMES.len()]
}

/// The TERM (shell) working spinner frame for the elapsed wall-clock time.
pub(crate) fn term_frame(elapsed: Duration) -> &'static str {
    let idx = (elapsed.as_millis() / FRAME_INTERVAL_MS) as usize;
    TERM_FRAMES[idx % TERM_FRAMES.len()]
}

/// The MCP tool-call spinner frame for the elapsed wall-clock time.
pub(crate) fn mcp_frame(elapsed: Duration) -> &'static str {
    let idx = (elapsed.as_millis() / FRAME_INTERVAL_MS) as usize;
    MCP_FRAMES[idx % MCP_FRAMES.len()]
}

/// The spinner frame for work robco itself is running right now (dropr:545).
pub(crate) fn robco_frame(elapsed: Duration) -> &'static str {
    let idx = (elapsed.as_millis() / FRAME_INTERVAL_MS) as usize;
    ROBCO_FRAMES[idx % ROBCO_FRAMES.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The point of a spinner is that it moves. A frame set that repeats
    /// within one cycle would sit still for part of it.
    #[test]
    fn robco_frames_advance_and_wrap() {
        let first = robco_frame(Duration::ZERO);
        let second = robco_frame(FRAME_INTERVAL);
        assert_ne!(first, second);
        let full_cycle = FRAME_INTERVAL * ROBCO_FRAMES.len() as u32;
        assert_eq!(robco_frame(full_cycle), first);
    }

    /// robco's own work must not be mistaken for an agent's. Each spinner
    /// vocabulary stays disjoint from the others.
    #[test]
    fn robco_frames_share_no_glyph_with_the_agent_spinners() {
        for frame in ROBCO_FRAMES {
            assert!(!FRAMES.contains(&frame), "{frame}");
            assert!(!TERM_FRAMES.contains(&frame), "{frame}");
            assert!(!MCP_FRAMES.contains(&frame), "{frame}");
        }
    }
}
