use std::collections::{HashMap, VecDeque};

use super::command_gate::{describe_command, impactful, mutating};
use super::commands::{Command, Input, parse};
use crate::overseer::config::DiscordConfig;

pub trait CommandExecutor {
    fn execute(&mut self, command: &Command, user_id: &str) -> Result<String, String>;

    fn refused(&mut self, _command: &Command, _user_id: &str, _reason: &str) {}
}

#[derive(Debug, Clone)]
struct Pending {
    channel_id: String,
    user_id: String,
    case_id: Option<String>,
    command: Command,
    expires_at: u64,
}

/// How a `Handled` result should be reported as a Discord reaction. Distinct
/// from `succeeded`/`executed` because those two alone cannot tell a
/// still-pending confirmation prompt (not terminal) apart from a rate-limit
/// refusal (terminal) — both leave `executed: None, succeeded: false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandledOutcome {
    Success,
    Failure,
    Refused,
    AwaitingConfirmation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Handled {
    pub response: String,
    pub executed: Option<Command>,
    pub case_id: Option<String>,
    pub succeeded: bool,
    pub outcome: HandledOutcome,
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

    #[cfg(test)]
    pub fn handle(
        &mut self,
        channel_id: &str,
        user_id: &str,
        message: &str,
        now_secs: u64,
        executor: &mut dyn CommandExecutor,
    ) -> Option<String> {
        self.handle_allowed(channel_id, user_id, message, now_secs, false, executor)
            .map(|handled| handled.response)
    }

    pub(crate) fn handle_allowed(
        &mut self,
        channel_id: &str,
        user_id: &str,
        message: &str,
        now_secs: u64,
        extra_channel: bool,
        executor: &mut dyn CommandExecutor,
    ) -> Option<Handled> {
        if (channel_id != self.channel_id && !extra_channel)
            || !self.allowed_users.iter().any(|allowed| allowed == user_id)
        {
            return None;
        }
        self.pending
            .retain(|_, pending| pending.expires_at >= now_secs);
        match parse(message)? {
            Input::Confirm(code) => self.confirm(&code, channel_id, user_id, now_secs, executor),
            Input::Command(command) if impactful(&command) => {
                Some(self.queue_confirmation(channel_id, user_id, None, command, now_secs))
            }
            Input::Command(command) => Some(self.execute(command, user_id, now_secs, executor)),
        }
    }

    pub(crate) fn submit_generated(
        &mut self,
        channel_id: &str,
        user_id: &str,
        case_id: Option<String>,
        command: Command,
        now_secs: u64,
        executor: &mut dyn CommandExecutor,
    ) -> Handled {
        if impactful(&command) {
            self.queue_confirmation(channel_id, user_id, case_id, command, now_secs)
        } else {
            self.execute(command, user_id, now_secs, executor)
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
        channel_id: &str,
        user_id: &str,
        now_secs: u64,
        executor: &mut dyn CommandExecutor,
    ) -> Option<Handled> {
        let Some(pending) = self.pending.remove(code) else {
            return Some(Handled {
                response: "confirmation rejected".into(),
                executed: None,
                case_id: None,
                succeeded: false,
                outcome: HandledOutcome::Refused,
            });
        };
        if pending.user_id != user_id
            || pending.channel_id != channel_id
            || pending.expires_at < now_secs
        {
            return Some(Handled {
                response: "confirmation rejected".into(),
                executed: None,
                case_id: None,
                succeeded: false,
                outcome: HandledOutcome::Refused,
            });
        }
        let mut handled = self.execute(pending.command, user_id, now_secs, executor);
        handled.case_id = pending.case_id;
        Some(handled)
    }

    fn execute(
        &mut self,
        command: Command,
        user_id: &str,
        now_secs: u64,
        executor: &mut dyn CommandExecutor,
    ) -> Handled {
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
                return Handled {
                    response: "rate limit exceeded; try again later".into(),
                    executed: None,
                    case_id: None,
                    succeeded: false,
                    outcome: HandledOutcome::Refused,
                };
            }
            self.actions.push_back(now_secs);
        }
        let result = executor.execute(&command, user_id);
        let succeeded = result.is_ok();
        let response = result.unwrap_or_else(|error| format!("error: {error}"));
        Handled {
            response,
            executed: Some(command),
            case_id: None,
            succeeded,
            outcome: if succeeded {
                HandledOutcome::Success
            } else {
                HandledOutcome::Failure
            },
        }
    }

    fn queue_confirmation(
        &mut self,
        channel_id: &str,
        user_id: &str,
        case_id: Option<String>,
        command: Command,
        now_secs: u64,
    ) -> Handled {
        let code = nanoid::nanoid!(8);
        let response = format!(
            "Confirm: {} — reply `CONFIRM {code}`",
            describe_command(&command)
        );
        self.pending.insert(
            code,
            Pending {
                channel_id: channel_id.into(),
                user_id: user_id.into(),
                case_id,
                command,
                expires_at: now_secs.saturating_add(self.confirmation_ttl_secs),
            },
        );
        Handled {
            response,
            executed: None,
            case_id: None,
            succeeded: false,
            outcome: HandledOutcome::AwaitingConfirmation,
        }
    }
}

#[cfg(test)]
#[path = "handler_tests.rs"]
mod tests;
