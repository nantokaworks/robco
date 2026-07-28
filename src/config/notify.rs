//! Per-status desktop/terminal notification toggles for robco's own agent
//! status changes — unrelated to the Overseer's Discord notifications in
//! [`crate::overseer::config::DiscordConfig`].

use serde::{Deserialize, Serialize};

use super::notify_flag_default;
use crate::model::Status;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotifyConfig {
    pub enabled: bool,
    pub waiting: bool,
    pub idle: bool,
    /// Notify when the AI finishes a turn (`Status::Done`). Defaults to on.
    #[serde(default = "notify_flag_default")]
    pub done: bool,
    pub dead: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            waiting: true,
            idle: true,
            done: true,
            dead: true,
        }
    }
}

impl NotifyConfig {
    pub fn wants(&self, status: Status) -> bool {
        if !self.enabled {
            return false;
        }

        match status {
            Status::Waiting => self.waiting,
            Status::Idle => self.idle,
            Status::Done => self.done,
            Status::Dead => self.dead,
            Status::Running | Status::BranchOnly => false,
        }
    }
}
