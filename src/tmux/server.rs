use std::{path::PathBuf, process::Command};

/// Which tmux server a command should reach.
///
/// Every function in this module takes one explicitly instead of reading a
/// server choice from the environment at call time: an env var read per-command
/// is process-global, so two tests racing to isolate themselves under
/// `--test-threads` would stomp on each other's choice — the same class of
/// flake dropr:549 already covers. Passing the server as a plain argument has
/// no such shared state to race on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TmuxServer(Option<PathBuf>);

impl TmuxServer {
    /// The operator's own default tmux server — what every call reached
    /// before this type existed. Command construction is byte-for-byte
    /// unchanged: no `-S` argument is added at all.
    pub fn default_server() -> Self {
        Self(None)
    }

    /// Crate-visible rather than module-private: a few call sites outside
    /// `src/tmux` (the daemon's own liveness/activity probes) build a raw
    /// `tmux` command directly instead of going through this module's own
    /// session helpers, and still need to target the same server.
    pub(crate) fn command(&self) -> Command {
        let mut command = Command::new("tmux");
        if let Some(socket) = &self.0 {
            command.arg("-S").arg(socket);
        }
        command
    }
}

#[cfg(test)]
impl TmuxServer {
    /// A throwaway server reachable only via a UNIX socket at `socket`. Keep
    /// the path short: a path under a deep scratch directory is longer than
    /// the ~104-byte UNIX socket limit and fails with "File name too long".
    fn socket(socket: impl Into<PathBuf>) -> Self {
        Self(Some(socket.into()))
    }

    /// One throwaway server, shared by every test in this process, that
    /// never reaches the operator's own tmux server (dropr:555). Named from
    /// the test process's own pid under `/tmp` directly rather than
    /// `std::env::temp_dir()`: `TMPDIR` on macOS resolves under
    /// `/var/folders/...`, and stacked with a scratch-directory prefix that
    /// can already push past the ~104-byte `sockaddr_un` path limit.
    pub(crate) fn for_tests() -> Self {
        Self::socket(format!("/tmp/robco-test-tmux-{}.sock", std::process::id()))
    }
}
