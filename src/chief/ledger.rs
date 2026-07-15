use std::{fs, io::ErrorKind, path::Path};

use chrono::{DateTime, NaiveDate, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LedgerEntry {
    pub task_id: String,
    pub display_id: String,
    pub repo: String,
    pub agent_id: String,
    pub branch: String,
    pub phase: LedgerPhase,
    pub dispatched_at: DateTime<Utc>,
    pub retries: u32,
    pub pr_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerPhase {
    Dispatched,
    Claimed,
    Working,
    PrOpened,
    Merged,
    Failed,
    Escalated,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LedgerCounters {
    pub date: Option<NaiveDate>,
    pub dispatched_today: u32,
    pub consecutive_failures: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Ledger {
    pub entries: Vec<LedgerEntry>,
    pub skip_list: Vec<String>,
    pub counters: LedgerCounters,
}

impl Ledger {
    pub fn load() -> Result<Self> {
        let path = super::ledger_path()?;
        Self::load_from(&path)
    }

    pub fn save(&self) -> Result<()> {
        let path = super::ledger_path()?;
        Self::save_to(self, &path)
    }

    fn load_from(path: &Path) -> Result<Self> {
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.into()),
        };
        match serde_json::from_str(&raw) {
            Ok(ledger) => Ok(ledger),
            Err(error) => {
                let corrupt_path = path.with_extension("json.corrupt");
                fs::rename(path, &corrupt_path)?;
                eprintln!(
                    "warning: corrupt chief ledger {} moved to {}; using defaults: {error}",
                    path.display(),
                    corrupt_path.display()
                );
                Ok(Self::default())
            }
        }
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        let temp_path = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let written = fs::write(&temp_path, raw).and_then(|()| fs::rename(&temp_path, path));
        if let Err(error) = written {
            let _ = fs::remove_file(temp_path);
            return Err(error.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("chief/ledger.json");
        let ledger = Ledger {
            entries: vec![LedgerEntry {
                task_id: "task-1".into(),
                display_id: "#128".into(),
                repo: "nantokaworks/robco".into(),
                agent_id: "worker-1".into(),
                branch: "task-128".into(),
                phase: LedgerPhase::PrOpened,
                dispatched_at: Utc::now(),
                retries: 1,
                pr_url: Some("https://example.test/pr/1".into()),
            }],
            skip_list: vec!["task-2".into()],
            counters: LedgerCounters {
                date: Some(Utc::now().date_naive()),
                dispatched_today: 2,
                consecutive_failures: 1,
            },
        };

        ledger.save_to(&path).unwrap();
        let serialized = fs::read_to_string(&path).unwrap();
        assert!(serialized.contains("\"pr_opened\""));
        assert_eq!(Ledger::load_from(&path).unwrap(), ledger);
    }

    #[test]
    fn phases_serialize_to_required_strings() {
        let phases = [
            (LedgerPhase::Dispatched, "dispatched"),
            (LedgerPhase::Claimed, "claimed"),
            (LedgerPhase::Working, "working"),
            (LedgerPhase::PrOpened, "pr_opened"),
            (LedgerPhase::Merged, "merged"),
            (LedgerPhase::Failed, "failed"),
            (LedgerPhase::Escalated, "escalated"),
        ];

        for (phase, expected) in phases {
            assert_eq!(
                serde_json::to_string(&phase).unwrap(),
                format!("\"{expected}\"")
            );
        }
    }

    #[test]
    fn missing_ledger_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ledger.json");
        assert_eq!(Ledger::load_from(&path).unwrap(), Ledger::default());
    }

    #[test]
    fn corrupt_ledger_is_preserved_aside() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ledger.json");
        fs::write(&path, "not json").unwrap();
        assert_eq!(Ledger::load_from(&path).unwrap(), Ledger::default());
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(path.with_extension("json.corrupt")).unwrap(),
            "not json"
        );
    }
}
