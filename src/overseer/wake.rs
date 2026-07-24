//! Waking the Overseer daemon between poll ticks.
//!
//! [`super::runtime_request::enqueue`] drops a request file into
//! `~/.robco/overseer/runtime_requests/`, which the daemon drains at the top of
//! every pass — but nothing told the daemon the file was there, so the request
//! waited out the rest of `poll_interval_secs`. The enqueuing process now rings
//! a bell: a signal to the pid in the daemon pidfile ends the sleep early, and
//! the pass that drains the queue starts within a couple of seconds.
//!
//! The bell is `SIGURG` rather than the more obvious `SIGUSR1` because its
//! default disposition is *ignore*. Requests are enqueued by other processes
//! from a pidfile that can be stale — a recycled pid, or a daemon from a build
//! that predates this handler, ignores `SIGURG` and would have been terminated
//! by `SIGUSR1`. Nothing in RobCo generates `SIGURG` otherwise: it is raised for
//! out-of-band socket data only on a socket whose owner was set explicitly,
//! which no part of this program does.

use std::time::{Duration, Instant};

use crate::{Result, overseer::logging, overseer::pidfile_path};

/// Floor between the start of one pass and the start of the next. Requests are
/// enqueued by other processes, so without a floor a caller could drive the
/// loop as fast as it can write files. Requests arriving inside the floor are
/// not lost — they stay in the queue, and the pass the floor releases drains
/// every one of them, so a burst costs one pass rather than one pass each.
pub(crate) const MIN_PASS_INTERVAL: Duration = Duration::from_secs(2);

/// Why the daemon stopped waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wake {
    /// The poll interval expired.
    Elapsed,
    /// A runtime request asked for a pass now.
    Requested,
    /// SIGINT or SIGTERM.
    Shutdown,
}

/// Waits out the remainder of the poll interval. Returns `true` when the daemon
/// was asked to shut down, so the caller stops the loop exactly as it did when
/// the wait could only end on a signal.
pub(crate) async fn wait_for_next_pass(
    signals: &mut Signals,
    started: Instant,
    poll_interval: Duration,
) -> Result<bool> {
    match signals
        .wait(poll_interval.saturating_sub(started.elapsed()))
        .await
    {
        Wake::Shutdown => return Ok(true),
        Wake::Elapsed => return Ok(false),
        Wake::Requested => {}
    }
    let hold = hold_for(started.elapsed());
    if hold.is_zero() {
        return Ok(false);
    }
    logging::log_message(
        None,
        &format!(
            "runtime request wake held {}ms to keep passes {}s apart; requests arriving meanwhile \
             coalesce into the pass that follows",
            hold.as_millis(),
            MIN_PASS_INTERVAL.as_secs()
        ),
    )?;
    Ok(signals.settle(hold).await)
}

/// How long a wake must be held so two passes stay [`MIN_PASS_INTERVAL`] apart,
/// given how long the pass being woken from already took.
fn hold_for(elapsed: Duration) -> Duration {
    MIN_PASS_INTERVAL.saturating_sub(elapsed)
}

/// Ask the daemon to start its next pass now. Best effort by design: with no
/// daemon running, a stale pidfile, or a daemon too old to listen, the enqueued
/// request simply waits for the next poll — which is the behaviour this
/// replaces, not a regression of it.
#[cfg(unix)]
pub(crate) fn notify_daemon() {
    let Some(pid) = daemon_pid() else {
        return;
    };
    wake_pid(pid);
}

#[cfg(not(unix))]
pub(crate) fn notify_daemon() {}

#[cfg(unix)]
fn daemon_pid() -> Option<u32> {
    std::fs::read_to_string(pidfile_path().ok()?)
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(unix)]
fn wake_pid(pid: u32) {
    // SAFETY: `kill` borrows no memory and its only side effect here is a signal
    // whose default disposition is ignore, so a pid that no longer belongs to a
    // daemon is a no-op rather than a casualty.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGURG) };
}

/// The signal handlers the poll loop waits on, registered once for the life of
/// the daemon. Registering them per wait would drop every signal delivered
/// while a pass was running — including the wake, which by its nature arrives
/// while the daemon is busy doing the thing the requester wants redone.
#[cfg(unix)]
pub(crate) struct Signals {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
    wake: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Signals {
    pub(crate) fn install() -> Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
            wake: signal(SignalKind::from_raw(libc::SIGURG))?,
        })
    }

    async fn wait(&mut self, duration: Duration) -> Wake {
        tokio::select! {
            _ = tokio::time::sleep(duration) => Wake::Elapsed,
            _ = self.interrupt.recv() => Wake::Shutdown,
            _ = self.terminate.recv() => Wake::Shutdown,
            _ = self.wake.recv() => Wake::Requested,
        }
    }

    /// Sleeps out the floor between passes, deliberately deaf to wakes so a
    /// request arriving inside it cannot shorten it. The signal is still
    /// recorded, so that request ends the *next* wait immediately instead of
    /// being lost.
    async fn settle(&mut self, duration: Duration) -> bool {
        tokio::select! {
            _ = tokio::time::sleep(duration) => false,
            _ = self.interrupt.recv() => true,
            _ = self.terminate.recv() => true,
        }
    }
}

/// Off unix there is no wake path — [`notify_daemon`] is a no-op — so the loop
/// keeps its timer-and-ctrl-c behaviour. A `ctrl_c()` that fails is treated as
/// a shutdown: the daemon cannot observe interrupts any more, and continuing
/// would leave it unstoppable.
#[cfg(not(unix))]
pub(crate) struct Signals;

#[cfg(not(unix))]
impl Signals {
    pub(crate) fn install() -> Result<Self> {
        Ok(Self)
    }

    async fn wait(&mut self, duration: Duration) -> Wake {
        tokio::select! {
            _ = tokio::time::sleep(duration) => Wake::Elapsed,
            _ = tokio::signal::ctrl_c() => Wake::Shutdown,
        }
    }

    async fn settle(&mut self, duration: Duration) -> bool {
        tokio::select! {
            _ = tokio::time::sleep(duration) => false,
            _ = tokio::signal::ctrl_c() => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_covers_the_floor_a_short_pass_left_open() {
        assert_eq!(
            hold_for(Duration::from_millis(500)),
            MIN_PASS_INTERVAL - Duration::from_millis(500)
        );
    }

    #[test]
    fn a_pass_longer_than_the_floor_holds_nothing() {
        assert!(hold_for(MIN_PASS_INTERVAL).is_zero());
        assert!(hold_for(MIN_PASS_INTERVAL * 3).is_zero());
    }

    // One test rather than three: the wake is a process-wide signal, so
    // concurrently running tests would observe each other's raises.
    #[cfg(unix)]
    #[tokio::test]
    async fn wake_starts_a_pass_early_and_the_floor_absorbs_the_rest() {
        let mut signals = Signals::install().unwrap();

        // A wake ends a wait that would otherwise run for a whole poll interval.
        wake_pid(std::process::id());
        let started = Instant::now();
        assert_eq!(signals.wait(Duration::from_secs(60)).await, Wake::Requested);
        assert!(started.elapsed() < Duration::from_secs(5));

        // Without one, the wait still runs to its deadline.
        assert_eq!(signals.wait(Duration::from_millis(50)).await, Wake::Elapsed);

        // The floor does not end early on a wake...
        wake_pid(std::process::id());
        let started = Instant::now();
        assert!(!signals.settle(Duration::from_millis(100)).await);
        assert!(started.elapsed() >= Duration::from_millis(100));

        // ...and the wake it absorbed still starts the pass after it.
        assert_eq!(signals.wait(Duration::from_secs(60)).await, Wake::Requested);
    }
}
