use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{config::NotifyConfig, model::Status};

#[derive(Debug, Clone)]
pub struct WatchTarget {
    pub tmux_session: String,
    pub label: String,
    pub status: Status,
}

#[derive(Debug, Default)]
struct WatchEntry {
    previous: Option<Status>,
}

struct Notifier {
    stdout_tx: mpsc::Sender<String>,
}

impl Notifier {
    fn notify(&self, title: &str, body: &str) {
        let payload = osc777(title, body);
        let ttys = attached_client_ttys();

        if ttys.is_empty() {
            let _ = self.stdout_tx.send(payload);
            return;
        }

        for tty in ttys {
            if let Ok(mut file) = OpenOptions::new().write(true).open(tty) {
                let _ = file.write_all(payload.as_bytes());
                let _ = file.flush();
            }
        }
    }
}

pub fn osc777(title: &str, body: &str) -> String {
    format!("\x1b]777;notify;{title};{body}\x07")
}

pub fn attached_client_ttys() -> Vec<String> {
    let Ok(output) = Command::new("tmux")
        .args(["list-clients", "-F", "#{client_tty}"])
        .output()
    else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut ttys = Vec::new();
    for tty in text.lines().map(str::trim).filter(|tty| !tty.is_empty()) {
        if !ttys.iter().any(|seen| seen == tty) {
            ttys.push(tty.to_string());
        }
    }
    ttys
}

pub fn spawn_watcher(
    targets: Arc<Mutex<Vec<WatchTarget>>>,
    notify_cfg: NotifyConfig,
    stdout_tx: mpsc::Sender<String>,
    running: Arc<AtomicBool>,
    poll_interval: Duration,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let notifier = Notifier { stdout_tx };
        let mut entries = HashMap::<String, WatchEntry>::new();

        while running.load(Ordering::Relaxed) {
            let snapshot = targets
                .lock()
                .map(|targets| targets.clone())
                .unwrap_or_default();
            let mut live_sessions = Vec::with_capacity(snapshot.len());

            for target in snapshot {
                live_sessions.push(target.tmux_session.clone());
                let entry = entries.entry(target.tmux_session.clone()).or_default();
                let current = target.status;

                if should_notify_transition(entry.previous, current, notify_cfg) {
                    notifier.notify(
                        "robco",
                        &format!("{}: {}", target.label, human_status(current)),
                    );
                }
                entry.previous = Some(current);
            }

            entries.retain(|session, _| live_sessions.iter().any(|live| live == session));
            sleep_while_running(&running, poll_interval);
        }
    })
}

pub fn human_status(status: Status) -> &'static str {
    match status {
        Status::Waiting => "waiting for input",
        Status::Idle => "idle / finished",
        Status::Dead => "session died",
        Status::Running => "running",
        Status::BranchOnly => "branch only",
    }
}

fn should_notify_transition(
    previous: Option<Status>,
    current: Status,
    notify_cfg: NotifyConfig,
) -> bool {
    previous
        .map(|previous| previous != current && notify_cfg.wants(current))
        .unwrap_or(false)
}

fn sleep_while_running(running: &AtomicBool, duration: Duration) {
    let started = Instant::now();
    while running.load(Ordering::Relaxed) && started.elapsed() < duration {
        let remaining = duration.saturating_sub(started.elapsed());
        thread::sleep(remaining.min(Duration::from_millis(50)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc777_builds_exact_sequence() {
        assert_eq!(
            osc777("robco", "agent: waiting"),
            "\x1b]777;notify;robco;agent: waiting\x07"
        );
    }

    #[test]
    fn notify_config_wants_only_enabled_terminal_states() {
        let cfg = NotifyConfig::default();
        assert!(cfg.wants(Status::Waiting));
        assert!(cfg.wants(Status::Idle));
        assert!(cfg.wants(Status::Dead));
        assert!(!cfg.wants(Status::Running));
        assert!(!cfg.wants(Status::BranchOnly));

        let cfg = NotifyConfig {
            waiting: false,
            idle: true,
            dead: false,
            ..NotifyConfig::default()
        };
        assert!(!cfg.wants(Status::Waiting));
        assert!(cfg.wants(Status::Idle));
        assert!(!cfg.wants(Status::Dead));

        assert!(
            !NotifyConfig {
                enabled: false,
                ..NotifyConfig::default()
            }
            .wants(Status::Idle)
        );
    }

    #[test]
    fn transition_predicate_only_fires_on_wanted_change() {
        let cfg = NotifyConfig::default();
        assert!(!should_notify_transition(None, Status::Waiting, cfg));
        assert!(!should_notify_transition(
            Some(Status::Waiting),
            Status::Waiting,
            cfg
        ));
        assert!(should_notify_transition(
            Some(Status::Running),
            Status::Waiting,
            cfg
        ));
        assert!(!should_notify_transition(
            Some(Status::Idle),
            Status::Running,
            cfg
        ));
    }
}
