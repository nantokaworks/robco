use crate::chief::triage::ExceptionCase;
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, fs, io::ErrorKind, path::Path};

pub(super) const MAX_DELIVERY_ATTEMPTS: u8 = 5;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResolutionState {
    #[default]
    Open,
    Pending,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct ThreadCase {
    pub case: ExceptionCase,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub opening_delivered: bool,
    #[serde(default)]
    pub resolution: ResolutionState,
    #[serde(default)]
    pub resolution_posted: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub delivery_attempts: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DeliveryOutcome {
    Retry,
    Complete,
    Exhausted,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct OpsState {
    cases: HashMap<String, ThreadCase>,
}

impl OpsState {
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(raw) => serde_json::from_slice(&raw).map_err(|error| error.to_string()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn reconcile_cases(&mut self, path: &Path, triage_root: &Path) -> Result<(), String> {
        let entries = match fs::read_dir(triage_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        let mut changed = false;
        for entry in entries.flatten() {
            let dir = entry.path();
            let Ok(raw) = fs::read(dir.join("case.json")) else {
                continue;
            };
            let Ok(case) = serde_json::from_slice::<ExceptionCase>(&raw) else {
                continue;
            };
            if !self.cases.contains_key(&case.id) && escalated(&dir.join("outcome.json")) {
                self.cases.insert(
                    case.id.clone(),
                    ThreadCase {
                        case,
                        thread_id: None,
                        opening_delivered: false,
                        resolution: ResolutionState::Open,
                        resolution_posted: false,
                        archived: false,
                        delivery_attempts: 0,
                    },
                );
                changed = true;
            }
        }
        if changed { self.save(path) } else { Ok(()) }
    }

    pub fn cases(&self) -> impl Iterator<Item = &ThreadCase> {
        self.cases.values()
    }

    pub fn by_thread(&self, thread_id: &str) -> Option<&ThreadCase> {
        self.cases
            .values()
            .find(|case| case.thread_id.as_deref() == Some(thread_id))
    }

    pub fn map_thread(
        &mut self,
        path: &Path,
        case_id: &str,
        thread_id: String,
    ) -> Result<(), String> {
        let case = self.cases.get_mut(case_id).ok_or("unknown case intent")?;
        case.thread_id = Some(thread_id);
        self.save(path)
    }

    pub fn mark_opening_delivered(&mut self, path: &Path, case_id: &str) -> Result<(), String> {
        let case = self.cases.get_mut(case_id).ok_or("unknown case intent")?;
        case.opening_delivered = true;
        self.save(path)
    }

    pub fn begin_resolution(&mut self, path: &Path, case_id: &str) -> Result<bool, String> {
        let case = self.cases.get_mut(case_id).ok_or("unknown case intent")?;
        if case.resolution != ResolutionState::Open {
            return Ok(false);
        }
        case.resolution = ResolutionState::Pending;
        self.save(path)?;
        Ok(true)
    }

    pub fn record_delivery(
        &mut self,
        path: &Path,
        case_id: &str,
        posted: bool,
        archived: bool,
    ) -> Result<DeliveryOutcome, String> {
        let case = self.cases.get_mut(case_id).ok_or("unknown case intent")?;
        if case.resolution != ResolutionState::Pending {
            return Ok(DeliveryOutcome::Complete);
        }
        case.resolution_posted |= posted;
        case.archived |= archived;
        case.delivery_attempts = case.delivery_attempts.saturating_add(1);
        let outcome = if case.resolution_posted && case.archived {
            case.resolution = ResolutionState::Resolved;
            DeliveryOutcome::Complete
        } else if case.delivery_attempts >= MAX_DELIVERY_ATTEMPTS {
            DeliveryOutcome::Exhausted
        } else {
            DeliveryOutcome::Retry
        };
        self.save(path)?;
        Ok(outcome)
    }

    pub fn finalize_exhausted(&mut self, path: &Path, case_id: &str) -> Result<(), String> {
        let case = self.cases.get_mut(case_id).ok_or("unknown case intent")?;
        if case.resolution == ResolutionState::Pending
            && case.delivery_attempts >= MAX_DELIVERY_ATTEMPTS
        {
            case.resolution = ResolutionState::Resolved;
            self.save(path)?;
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let temp = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let raw = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        if let Err(error) = fs::write(&temp, raw).and_then(|()| fs::rename(&temp, path)) {
            let _ = fs::remove_file(temp);
            return Err(error.to_string());
        }
        Ok(())
    }
}

fn escalated(path: &Path) -> bool {
    fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|value| value.get("outcome")?.as_str().map(str::to_owned))
        .as_deref()
        == Some("escalate")
}
