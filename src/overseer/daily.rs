//! A per-UTC-date call counter persisted next to the surface that owns it.
//!
//! Every LLM surface gets its own file and its own budget. Sharing one counter
//! would let a surface on a fixed cadence — the board reviewer runs whether or
//! not there is work — starve an on-demand one.

use crate::Result;
use chrono::{NaiveDate, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use std::{fs, io::ErrorKind, path::Path};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DailyCounter {
    date: Option<NaiveDate>,
    count: u32,
}

impl DailyCounter {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(raw) => Ok(serde_json::from_slice(&raw).unwrap_or_default()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn count_today(&self) -> u32 {
        if self.date == Some(Utc::now().date_naive()) {
            self.count
        } else {
            0
        }
    }

    pub(crate) fn increment(&mut self, path: &Path) -> Result<()> {
        let mut next = self.clone();
        let today = Utc::now().date_naive();
        if next.date != Some(today) {
            next.date = Some(today);
            next.count = 0;
        }
        next.count = next.count.saturating_add(1);
        save(path, &next)?;
        *self = next;
        Ok(())
    }
}

fn save(path: &Path, counter: &DailyCounter) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("json.{}.tmp", nanoid!()));
    let raw = serde_json::to_vec_pretty(counter)?;
    if let Err(error) = fs::write(&temp, raw).and_then(|()| fs::rename(&temp, path)) {
        let _ = fs::remove_file(temp);
        return Err(error.into());
    }
    Ok(())
}
