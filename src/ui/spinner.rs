use std::time::Duration;

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAME_INTERVAL_MS: u128 = 120;

/// Pick the animation frame from elapsed wall-clock time, so the spinner runs
/// at a steady rate regardless of how often the UI is redrawn.
pub(crate) fn frame(elapsed: Duration) -> &'static str {
    let idx = (elapsed.as_millis() / FRAME_INTERVAL_MS) as usize;
    FRAMES[idx % FRAMES.len()]
}
