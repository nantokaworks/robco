use std::collections::{HashMap, VecDeque};

use super::commands::{Command, Input, parse};
use crate::chief::config::DiscordConfig;

pub trait CommandExecutor {
    fn execute(&mut self, command: &Command, user_id: &str) -> Result<String, String>;

    fn refused(&mut self, _command: &Command, _user_id: &str, _reason: &str) {}
}

#[derive(Debug, Clone)]
struct Pending {
    user_id: String,
    command: Command,
    expires_at: u64,
}

pub struct Handler {
    channel_id: String,
    allowed_users: Vec<String>,
    confirmation_ttl_secs: u64,
    action_limit: usize,
    pending: HashMap<String, Pending>,
    actions: VecDeque<u64>,
}

impl Handler {
    pub fn new(
        channel_id: String,
        allowed_users: Vec<String>,
        confirmation_ttl_secs: u64,
        action_limit: usize,
    ) -> Self {
        Self {
            channel_id,
            allowed_users,
            confirmation_ttl_secs,
            action_limit,
            pending: HashMap::new(),
            actions: VecDeque::new(),
        }
    }

    pub fn handle(
        &mut self,
        channel_id: &str,
        user_id: &str,
        message: &str,
        now_secs: u64,
        executor: &mut dyn CommandExecutor,
    ) -> Option<String> {
        if channel_id != self.channel_id
            || !self.allowed_users.iter().any(|allowed| allowed == user_id)
        {
            return None;
        }
        self.pending
            .retain(|_, pending| pending.expires_at >= now_secs);
        match parse(message)? {
            Input::Confirm(code) => self.confirm(&code, user_id, now_secs, executor),
            Input::Command(command) if impactful(&command) => {
                let code = nanoid::nanoid!(8);
                self.pending.insert(
                    code.clone(),
                    Pending {
                        user_id: user_id.into(),
                        command,
                        expires_at: now_secs.saturating_add(self.confirmation_ttl_secs),
                    },
                );
                Some(format!("reply `CONFIRM {code}` to execute"))
            }
            Input::Command(command) => self.execute(command, user_id, now_secs, executor),
        }
    }

    pub fn update_config(&mut self, config: &DiscordConfig) {
        self.channel_id = config.channel_id.clone().unwrap_or_default();
        self.allowed_users.clone_from(&config.allowed_user_ids);
        self.confirmation_ttl_secs = config.confirmation_ttl_secs;
        self.action_limit = config.action_limit_per_hour;
        self.pending
            .retain(|_, pending| self.allowed_users.contains(&pending.user_id));
    }

    fn confirm(
        &mut self,
        code: &str,
        user_id: &str,
        now_secs: u64,
        executor: &mut dyn CommandExecutor,
    ) -> Option<String> {
        let Some(pending) = self.pending.remove(code) else {
            return Some("confirmation rejected".into());
        };
        if pending.user_id != user_id || pending.expires_at < now_secs {
            return Some("confirmation rejected".into());
        }
        self.execute(pending.command, user_id, now_secs, executor)
    }

    fn execute(
        &mut self,
        command: Command,
        user_id: &str,
        now_secs: u64,
        executor: &mut dyn CommandExecutor,
    ) -> Option<String> {
        if mutating(&command) {
            while self
                .actions
                .front()
                .is_some_and(|at| at.saturating_add(3600) <= now_secs)
            {
                self.actions.pop_front();
            }
            if self.actions.len() >= self.action_limit {
                executor.refused(&command, user_id, "rate limit exceeded");
                return Some("rate limit exceeded; try again later".into());
            }
            self.actions.push_back(now_secs);
        }
        Some(
            executor
                .execute(&command, user_id)
                .unwrap_or_else(|error| format!("error: {error}")),
        )
    }
}

fn impactful(command: &Command) -> bool {
    matches!(
        command,
        Command::Kill(_)
            | Command::Panic
            | Command::Retry(_)
            | Command::Skip(_)
            | Command::Approve(_)
            | Command::Answer { .. }
            | Command::Dispatch(true)
    )
    // Risk-reducing `dispatch off` and `automerge off` remain immediate so an
    // incident responder is never delayed by the confirmation round trip.
}

fn mutating(command: &Command) -> bool {
    !matches!(
        command,
        Command::Status | Command::Workers | Command::Tasks | Command::Log(_)
    )
}

#[cfg(test)]
#[path = "handler_tests.rs"]
mod tests;
