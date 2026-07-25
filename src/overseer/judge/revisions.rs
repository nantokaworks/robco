//! The terminal verdicts a pull request already received.
//!
//! A veto or an escalation is remembered against the change it was given for —
//! [`super::keys::merge_key`]'s fingerprint, not the head sha. Keying on the
//! head meant a mechanical base update made the gate re-ask a question it had
//! already refused; keying on the fingerprint means the refusal stands until
//! the worker actually changes something, and lifts the moment it does.

use crate::Result;
use nanoid::nanoid;
use std::{collections::HashMap, fs, io::ErrorKind, path::Path};

/// Merge identity (task + pull request) to the change fingerprint its terminal
/// verdict was given for.
#[derive(Clone, Default)]
pub(super) struct RevisionCache(HashMap<String, String>);

impl RevisionCache {
    pub(super) fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(raw) => Ok(Self(serde_json::from_slice(&raw).unwrap_or_default())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn matches(&self, identity: &str, fingerprint: &str) -> bool {
        self.0
            .get(identity)
            .is_some_and(|judged| judged == fingerprint)
    }

    pub(super) fn contains(&self, identity: &str) -> bool {
        self.0.contains_key(identity)
    }

    pub(super) fn remember(
        &mut self,
        path: &Path,
        identity: String,
        fingerprint: String,
    ) -> Result<()> {
        let mut next = self.clone();
        next.0.insert(identity, fingerprint);
        next.save(path)?;
        *self = next;
        Ok(())
    }

    pub(super) fn clear(&mut self, path: &Path, identity: &str) -> Result<()> {
        let mut next = self.clone();
        if next.0.remove(identity).is_some() {
            next.save(path)?;
            *self = next;
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let raw = serde_json::to_vec_pretty(&self.0)?;
        if let Err(error) = fs::write(&temp, raw).and_then(|()| fs::rename(&temp, path)) {
            let _ = fs::remove_file(temp);
            return Err(error.into());
        }
        Ok(())
    }
}
