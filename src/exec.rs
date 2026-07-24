use std::{
    io::Read,
    process::{Command, Output, Stdio},
    thread::JoinHandle,
    time::{Duration, Instant},
};

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

pub(crate) fn run_timeout(mut command: Command, timeout: Duration) -> std::io::Result<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
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
            let _ = child.kill();
            let _ = child.wait();
            // A surviving descendant may keep either pipe open indefinitely. Preserve the
            // deadline guarantee by detaching the readers; process-group cleanup is task #210.
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

        let reaping_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = match Command::new("ps").args(["-o", "stat=", "-p", pid]).output() {
                Ok(status) => status,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
                Err(error) => panic!("failed to inspect child {pid}: {error}"),
            };
            if status.stdout.is_empty() {
                break;
            }
            assert!(
                Instant::now() < reaping_deadline,
                "child {pid} was not reaped; process state: {}",
                String::from_utf8_lossy(&status.stdout).trim()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
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
