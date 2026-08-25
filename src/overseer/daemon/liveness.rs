use super::super::COMMAND_TIMEOUT;
use crate::{overseer::exec::run_timeout, tmux::TmuxServer};
use std::{io, thread, time::Duration};

const TMUX_PROBE_ATTEMPTS: usize = 3;
const TMUX_PROBE_RETRY_DELAY: Duration = Duration::from_millis(200);

pub(super) fn probe_session_status(server: &TmuxServer, session: &str) -> io::Result<bool> {
    let mut attempt = 0;
    let mut probe_error = None;
    let dead = session_confirmed_dead(TMUX_PROBE_ATTEMPTS, || {
        if attempt > 0 {
            thread::sleep(TMUX_PROBE_RETRY_DELAY);
        }
        attempt += 1;
        let mut command = server.command();
        command.args(["has-session", "-t", &format!("={session}")]);
        match run_timeout(command, COMMAND_TIMEOUT) {
            Ok(output) => output.status.success(),
            Err(error) => {
                probe_error = Some(error);
                // Stop probing; command errors retain the skipped-observation path.
                true
            }
        }
    });
    match probe_error {
        Some(error) => Err(error),
        None => Ok(dead),
    }
}

pub(super) fn session_confirmed_dead(
    attempts: usize,
    mut session_present: impl FnMut() -> bool,
) -> bool {
    attempts > 0 && (0..attempts).all(|_| !session_present())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_tmux_probe_miss_is_not_confirmed_dead() {
        let mut results = [false, true].into_iter();

        assert!(!session_confirmed_dead(3, || results.next().unwrap()));
    }

    #[test]
    fn sustained_tmux_probe_misses_are_confirmed_dead() {
        let mut results = [false, false, false].into_iter();

        assert!(session_confirmed_dead(3, || results.next().unwrap()));
    }

    #[test]
    fn zero_attempts_cannot_confirm_death() {
        assert!(!session_confirmed_dead(0, || false));
    }

    #[test]
    fn one_attempt_uses_the_single_probe_result() {
        assert!(session_confirmed_dead(1, || false));
        assert!(!session_confirmed_dead(1, || true));
    }
}
