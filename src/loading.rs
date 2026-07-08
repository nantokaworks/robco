use std::{
    io::{self, IsTerminal, Write},
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread::JoinHandle,
    time::Instant,
};

use crate::ui::spinner;

enum Update {
    Message(String),
    Stop,
}

/// Animated startup indicator on stderr, shown while repo discovery and the
/// dropr overlay load run before the TUI takes over the screen. Renders
/// nothing when stderr is not a terminal, and clears its line on drop so an
/// early error return never leaves a stale spinner behind.
pub struct Indicator {
    tx: Option<Sender<Update>>,
    handle: Option<JoinHandle<()>>,
}

impl Indicator {
    pub fn start(message: &str) -> Self {
        if !io::stderr().is_terminal() {
            return Self {
                tx: None,
                handle: None,
            };
        }

        let (tx, rx) = mpsc::channel();
        let mut text = message.to_string();
        let handle = std::thread::spawn(move || {
            let started = Instant::now();
            let mut err = io::stderr();
            loop {
                let frame = spinner::frame(started.elapsed());
                let _ = write!(err, "\r\x1b[2K{frame} {text}");
                let _ = err.flush();
                match rx.recv_timeout(spinner::FRAME_INTERVAL) {
                    Ok(Update::Message(next)) => text = next,
                    Ok(Update::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }
            }
            let _ = write!(err, "\r\x1b[2K");
            let _ = err.flush();
        });

        Self {
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    pub fn set_message(&self, message: &str) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Update::Message(message.to_string()));
        }
    }

    /// Stop the animation and clear the spinner line before other output
    /// (the TUI, `--list` results, or an error report) takes the terminal.
    pub fn finish(self) {}
}

impl Drop for Indicator {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Update::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_message_finish_does_not_panic() {
        let indicator = Indicator::start("Scanning repositories...");
        indicator.set_message("Loading dropr workspaces...");
        indicator.finish();
    }
}
