//! What the review pass remembers between daemon restarts.
//!
//! Two things, both about not repeating itself: when it last ran, so a daemon
//! that restarts every few minutes cannot review every start-up and spend its
//! whole budget by noon; and which findings are currently outstanding, so a
//! condition that persists for six hours is escalated once rather than every
//! interval.

use chrono::{DateTime, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use std::{fs, io::ErrorKind, path::Path};

use crate::Result;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(super) struct State {
    pub last_run: Option<DateTime<Utc>>,
    /// Finding keys the deterministic rules reported at the last review.
    pub outstanding: Vec<String>,
    /// Reviewer sentences escalated at the last completed session.
    pub reported: Vec<String>,
}

impl State {
    pub(super) fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(raw) => Ok(serde_json::from_slice(&raw).unwrap_or_default()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let raw = serde_json::to_vec_pretty(self)?;
        if let Err(error) = fs::write(&temp, raw).and_then(|()| fs::rename(&temp, path)) {
            let _ = fs::remove_file(temp);
            return Err(error.into());
        }
        Ok(())
    }

    /// Replaces one remembered set with `current`, returning the members that
    /// were not already outstanding — the ones worth escalating now.
    pub(super) fn newly_seen(remembered: &mut Vec<String>, current: Vec<String>) -> Vec<String> {
        let fresh = current
            .iter()
            .filter(|key| !remembered.contains(key))
            .cloned()
            .collect();
        *remembered = current;
        fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_condition_that_persists_is_escalated_once() {
        let mut remembered = Vec::new();
        assert_eq!(
            State::newly_seen(&mut remembered, vec!["a".into(), "b".into()]),
            ["a", "b"]
        );
        assert!(State::newly_seen(&mut remembered, vec!["a".into(), "b".into()]).is_empty());
        // It clears, then returns: that is a new occurrence, not the old one.
        assert!(State::newly_seen(&mut remembered, Vec::new()).is_empty());
        assert_eq!(State::newly_seen(&mut remembered, vec!["a".into()]), ["a"]);
    }

    #[test]
    fn state_round_trips_and_tolerates_a_missing_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("review/state.json");
        assert_eq!(State::load(&path).unwrap(), State::default());

        let state = State {
            last_run: Some(Utc::now()),
            outstanding: vec!["circuit:open".into()],
            reported: vec!["worktree collisions".into()],
        };
        state.save(&path).unwrap();
        assert_eq!(State::load(&path).unwrap(), state);
    }
}
