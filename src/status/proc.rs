//! Process-tree snapshot and child-command tracking for agent panes.
//!
//! Known limits: an auto-restarted MCP server can look like a late command;
//! daemonized children reparented to PID 1 are invisible; commands spawned in
//! an agent's first 60 seconds that exec out of their shell can be missed
//! (though a surviving shell catches them); Linux truncates `comm` to 15
//! characters, which is cosmetic.

use std::{collections::HashMap, path::Path, process::Command};

use crate::{Error, Result};

const LATE_SPAWN_SLACK_SECS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcEntry {
    pub pid: u32,
    pub ppid: u32,
    pub elapsed_secs: u64,
    pub comm: String,
}

#[derive(Debug, Default)]
pub struct ProcSnapshot {
    entries: HashMap<u32, ProcEntry>,
    children: HashMap<u32, Vec<u32>>,
}

impl ProcSnapshot {
    pub fn capture() -> Result<Self> {
        let output = Command::new("ps")
            .args(["-Ao", "pid=,ppid=,etime=,comm="])
            .output()?;
        if !output.status.success() {
            return Err(Error::Command {
                context: "ps process snapshot",
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let entries = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(parse_entry)
            .collect();
        Ok(Self::from_entries(entries))
    }

    pub fn from_entries(entries: Vec<ProcEntry>) -> Self {
        let mut by_pid = HashMap::with_capacity(entries.len());
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for entry in entries {
            children.entry(entry.ppid).or_default().push(entry.pid);
            by_pid.insert(entry.pid, entry);
        }
        for child_pids in children.values_mut() {
            child_pids.sort_unstable();
        }
        Self {
            entries: by_pid,
            children,
        }
    }

    pub fn contains(&self, pid: u32) -> bool {
        self.entries.contains_key(&pid)
    }

    pub fn tracked_command(&self, pane_pid: u32) -> Option<String> {
        let root_pid = self.agent_root(pane_pid)?;
        let root_age = self.entries.get(&root_pid)?.elapsed_secs;
        let mut stack = self
            .children
            .get(&root_pid)
            .into_iter()
            .flatten()
            .map(|&pid| (pid, 1usize, false))
            .collect::<Vec<_>>();
        let mut best_non_shell = None;
        let mut best_shell = None;

        while let Some((pid, depth, inherited)) = stack.pop() {
            let Some(entry) = self.entries.get(&pid) else {
                continue;
            };
            let shell = is_shell_name(&entry.comm);
            let late = root_age.saturating_sub(entry.elapsed_secs) >= LATE_SPAWN_SLACK_SECS;
            let tracked = inherited || shell || late;
            if tracked {
                let candidate = (depth, pid, normalized_name(&entry.comm));
                if shell {
                    update_deepest(&mut best_shell, candidate);
                } else {
                    update_deepest(&mut best_non_shell, candidate);
                }
            }
            if let Some(children) = self.children.get(&pid) {
                stack.extend(children.iter().map(|&child| (child, depth + 1, tracked)));
            }
        }

        best_non_shell.or(best_shell).map(|(_, _, name)| name)
    }

    fn agent_root(&self, pane_pid: u32) -> Option<u32> {
        let mut current = pane_pid;
        loop {
            let entry = self.entries.get(&current)?;
            if !is_shell_name(&entry.comm) {
                return Some(current);
            }
            current = self
                .children
                .get(&current)?
                .iter()
                .filter_map(|pid| self.entries.get(pid))
                .max_by_key(|entry| (entry.elapsed_secs, std::cmp::Reverse(entry.pid)))?
                .pid;
        }
    }
}

fn update_deepest(best: &mut Option<(usize, u32, String)>, candidate: (usize, u32, String)) {
    if best
        .as_ref()
        .is_none_or(|(depth, pid, _)| (candidate.0, candidate.1) > (*depth, *pid))
    {
        *best = Some(candidate);
    }
}

fn normalized_name(comm: &str) -> String {
    let trimmed = comm.trim().trim_start_matches('-');
    Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(trimmed)
        .to_string()
}

fn parse_entry(line: &str) -> Option<ProcEntry> {
    let mut fields = line.split_whitespace();
    Some(ProcEntry {
        pid: fields.next()?.parse().ok()?,
        ppid: fields.next()?.parse().ok()?,
        elapsed_secs: parse_etime(fields.next()?)?,
        comm: fields.next()?.to_string(),
    })
}

pub fn parse_etime(value: &str) -> Option<u64> {
    let (days, clock) = match value.split_once('-') {
        Some((days, clock)) => (days.parse::<u64>().ok()?, clock),
        None => (0, value),
    };
    let parts = clock
        .split(':')
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    let clock_secs = match parts.as_slice() {
        [minutes, seconds] if *seconds < 60 => minutes.checked_mul(60)?.checked_add(*seconds)?,
        [hours, minutes, seconds] if *minutes < 60 && *seconds < 60 => hours
            .checked_mul(3600)?
            .checked_add(minutes.checked_mul(60)?)?
            .checked_add(*seconds)?,
        _ => return None,
    };
    days.checked_mul(86_400)?.checked_add(clock_secs)
}

pub fn is_shell_name(cmd: &str) -> bool {
    matches!(
        normalized_name(cmd).as_str(),
        "zsh" | "bash" | "fish" | "sh" | "dash" | "ksh" | "tcsh" | "csh"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pid: u32, ppid: u32, age: u64, comm: &str) -> ProcEntry {
        ProcEntry {
            pid,
            ppid,
            elapsed_secs: age,
            comm: comm.into(),
        }
    }

    #[test]
    fn ignores_mcp_only_tree() {
        let snapshot = ProcSnapshot::from_entries(vec![
            entry(1, 0, 500, "claude"),
            entry(2, 1, 495, "node"),
            entry(3, 1, 490, "python"),
        ]);
        assert_eq!(snapshot.tracked_command(1), None);
    }

    #[test]
    fn nested_shell_command_picks_deepest_non_shell() {
        let snapshot = ProcSnapshot::from_entries(vec![
            entry(1, 0, 500, "claude"),
            entry(2, 1, 20, "bash"),
            entry(3, 2, 19, "cargo"),
        ]);
        assert_eq!(snapshot.tracked_command(1).as_deref(), Some("cargo"));
    }

    #[test]
    fn childless_shell_is_reported() {
        let snapshot =
            ProcSnapshot::from_entries(vec![entry(1, 0, 500, "claude"), entry(2, 1, 20, "bash")]);
        assert_eq!(snapshot.tracked_command(1).as_deref(), Some("bash"));
    }

    #[test]
    fn late_exec_is_tracked_but_early_spawn_is_not() {
        let late = ProcSnapshot::from_entries(vec![
            entry(1, 0, 500, "claude"),
            entry(2, 1, 439, "/usr/bin/cargo"),
        ]);
        assert_eq!(late.tracked_command(1).as_deref(), Some("cargo"));
        let early =
            ProcSnapshot::from_entries(vec![entry(1, 0, 500, "claude"), entry(2, 1, 441, "cargo")]);
        assert_eq!(early.tracked_command(1), None);
    }

    #[test]
    fn pane_wrapper_shell_finds_command_below_agent() {
        let snapshot = ProcSnapshot::from_entries(vec![
            entry(1, 0, 600, "zsh"),
            entry(2, 1, 590, "claude"),
            entry(3, 2, 585, "node"),
            entry(4, 2, 15, "bash"),
            entry(5, 4, 14, "pytest"),
        ]);
        assert_eq!(snapshot.tracked_command(1).as_deref(), Some("pytest"));
    }

    #[test]
    fn parses_supported_elapsed_formats() {
        assert_eq!(parse_etime("02:03"), Some(123));
        assert_eq!(parse_etime("01:02:03"), Some(3_723));
        assert_eq!(parse_etime("2-01:02:03"), Some(176_523));
        assert_eq!(parse_etime("garbage"), None);
    }

    #[test]
    fn recognizes_login_and_path_shell_names() {
        assert!(is_shell_name("-zsh"));
        assert!(is_shell_name("/bin/bash"));
        assert!(!is_shell_name("cargo"));
    }
}
