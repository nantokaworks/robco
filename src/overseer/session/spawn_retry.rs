use std::{
    process::{Child, Command},
    thread,
    time::Duration,
};

/// Spawn `command`, retrying a short number of times when the OS answers
/// `ExecutableFileBusy` (`ETXTBSY`). On Linux, the kernel can still deny an
/// exec on a file this same process just wrote and closed, even though the
/// write handle is gone by then: the write-deny bit on the inode can clear a
/// few milliseconds later than the `close()` call that dropped it. A short
/// retry rides out that window without hiding a real launch failure — a
/// non-busy error, or a busy error that never clears, still returns to the
/// caller as-is.
pub(crate) fn spawn_retrying_text_busy(command: &mut Command) -> std::io::Result<Child> {
    const MAX_ATTEMPTS: u32 = 5;
    const RETRY_DELAY: Duration = Duration::from_millis(5);
    let mut last_error = None;
    for _ in 0..MAX_ATTEMPTS {
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                last_error = Some(error);
                thread::sleep(RETRY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.expect("loop runs MAX_ATTEMPTS >= 1 times"))
}
