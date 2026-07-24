use std::{
    io::Read,
    process::{Child, Command, Output, Stdio},
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// How long a timed-out process group is given to react to `SIGTERM` before `SIGKILL`.
#[cfg(unix)]
const TERMINATION_GRACE: Duration = Duration::from_millis(200);

fn drain<R: Read + Send + 'static>(mut pipe: R) -> JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        pipe.read_to_end(&mut buffer)?;
        Ok(buffer)
    })
}

fn collect(reader: JoinHandle<std::io::Result<Vec<u8>>>) -> Vec<u8> {
    reader
        .join()
        .ok()
        .and_then(std::io::Result::ok)
        .unwrap_or_default()
}

/// Terminate a timed-out child and every descendant it spawned.
///
/// `Child::kill` only signals the direct child, so a `git`/`gh` invocation that forked an ssh
/// process, a credential helper or a remote helper would leave those descendants running after the
/// caller was already told the command timed out. The child leads its own process group (see
/// `run_timeout`), so signalling the negated pid reaches the whole subtree instead.
///
/// `SIGTERM` goes first so descendants can release locks and close connections; `SIGKILL` follows
/// after a short grace period for anything that ignored it. The child is deliberately reaped only
/// after the `SIGKILL`: reaping it earlier would free its pid, and the pid doubles as the group id.
#[cfg(unix)]
fn terminate_group(child: &mut Child) {
    let pgid = child.id() as libc::pid_t;
    // SAFETY: `killpg` is async-signal-safe and takes no pointers. `pgid` names the group created
    // for this child, which cannot be recycled while the unreaped leader still holds its pid.
    unsafe { libc::killpg(pgid, libc::SIGTERM) };
    std::thread::sleep(TERMINATION_GRACE);
    // SAFETY: as above.
    unsafe { libc::killpg(pgid, libc::SIGKILL) };
    let _ = child.wait();
}

#[cfg(not(unix))]
fn terminate_group(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub(crate) fn run_timeout(mut command: Command, timeout: Duration) -> std::io::Result<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    // Give every child its own process group so a timeout can signal its descendants too, rather
    // than restricting that to the network tier. Each caller pipes stdout/stderr and consumes the
    // result programmatically, so none of these children is meant to interact with the user, and
    // the caller that actually reaches these deadlines is the overseer daemon, which has no
    // controlling terminal at all.
    //
    // The cost is that a child no longer shares the terminal's foreground group: Ctrl-C on a
    // foreground `robco` does not reach it, and a child that tries to read the tty (an ssh
    // passphrase prompt, say) is stopped by SIGTTIN instead of prompting. Both degrade into the
    // deadline this function already enforces — the command times out and the whole group is
    // reaped — so the failure stays bounded and leaks nothing.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn()?;
    let stdout_reader = drain(
        child
            .stdout
            .take()
            .expect("stdout is piped before spawning the child"),
    );
    let stderr_reader = drain(
        child
            .stderr
            .take()
            .expect("stderr is piped before spawning the child"),
    );
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Output {
                status,
                stdout: collect(stdout_reader),
                stderr: collect(stderr_reader),
            });
        }
        if Instant::now() >= deadline {
            terminate_group(&mut child);
            // A descendant that escaped the group (by calling `setsid` itself) could still hold
            // either pipe open. Preserve the deadline guarantee by detaching the readers.
            drop(stdout_reader);
            drop(stderr_reader);
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "command timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wait for `pid` to disappear from the process table, allowing for the brief zombie window
    /// between the signal landing and the parent (or init) reaping the process.
    fn assert_process_gone(pid: &str, label: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = match Command::new("ps").args(["-o", "stat=", "-p", pid]).output() {
                Ok(status) => status,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
                Err(error) => panic!("failed to inspect {label} {pid}: {error}"),
            };
            if status.stdout.is_empty() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{label} {pid} survived the timeout; process state: {}",
                String::from_utf8_lossy(&status.stdout).trim()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[test]
    fn times_out_hung_command_promptly() {
        let pid_file = tempfile::NamedTempFile::new().unwrap();
        let started = Instant::now();
        let mut command = Command::new("sh");
        command
            .args(["-c", "echo $$ > \"$1\"; while :; do :; done", "sh"])
            .arg(pid_file.path());
        let error = run_timeout(command, Duration::from_millis(200)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));

        let pid = std::fs::read_to_string(pid_file.path()).unwrap();
        let pid = pid.trim();
        assert!(!pid.is_empty());

        assert_process_gone(pid, "child");
    }

    #[test]
    fn timeout_kills_forked_grandchild() {
        let pid_file = tempfile::NamedTempFile::new().unwrap();
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "sleep 30 & echo $! > \"$1\"; while :; do :; done",
                "sh",
            ])
            .arg(pid_file.path());

        let error = run_timeout(command, Duration::from_millis(300)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);

        let pid = std::fs::read_to_string(pid_file.path()).unwrap();
        let pid = pid.trim();
        assert!(!pid.is_empty(), "grandchild pid was not recorded");

        assert_process_gone(pid, "grandchild");
    }

    #[test]
    fn timeout_does_not_wait_for_descendant_pipe_writer() {
        let started = Instant::now();
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 6 & while :; do :; done"]);

        let error = run_timeout(command, Duration::from_millis(200)).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn drains_large_command_stdout() {
        let mut command = Command::new("dd");
        command.args(["if=/dev/zero", "bs=1024", "count=512"]);
        let output = run_timeout(command, Duration::from_secs(30)).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 512 * 1024);
    }

    #[test]
    fn preserves_fast_command_stdout() {
        let mut command = Command::new("echo");
        command.arg("hello");
        let output = run_timeout(command, Duration::from_secs(1)).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"hello\n");
    }
}
