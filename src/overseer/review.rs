//! The periodic board review: the only Overseer surface that reads its own
//! history.
//!
//! Every other surface answers one case at a time. The dispatch judge sees a
//! candidate list by construction; triage sees one failed worker; the merge
//! judge sees one diff. None of them can see that the same spawn failure has
//! now happened three times, or that nothing has merged for an hour, because
//! nothing was ever wrong enough in a single pass to be worth reporting.
//!
//! So this runs on its own clock rather than every poll, reads a bounded digest
//! of recent decisions and ledger state, and reports what it finds. Its
//! authority stops there: it may diagnose and escalate, and it has no path to
//! dispatch, merge, unblock, or write the ledger. `tick` takes the ledger by
//! shared reference, which is where that guarantee actually lives.
//!
//! The deterministic findings run on the review's clock whether or not a reviewer
//! model is configured. `review_profile` switches on the model stage alone: what
//! it adds is diagnosis, and detection that depended on it would be detection that
//! never ran on the default configuration.

mod briefing;
mod digest;
mod findings;
mod result;
mod state;

use chrono::{DateTime, Utc};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
    time::Duration,
};

use crate::{
    Result,
    config::{Config, Profile},
    overseer::{
        daily::DailyCounter,
        ledger::Ledger,
        logging::{self, DecisionEntry, DecisionKind},
        session::{
            EphemeralSession, SessionControl, SessionHandle, SessionResult, session_profile,
            terminate_stale_session,
        },
    },
};

use digest::Digest;
use findings::Finding;

/// `source` written on every decision this pass records. Also what the digest
/// filters out, so the review never reads itself back as evidence.
pub(crate) const SOURCE: &str = "review";

pub struct ReviewPass {
    root: PathBuf,
    log_path: PathBuf,
    counter_path: PathBuf,
    counter: DailyCounter,
    state_path: PathBuf,
    state: state::State,
    active: Option<SessionHandle>,
}

impl ReviewPass {
    pub fn load() -> Result<Self> {
        Self::new(super::review_dir()?, super::decision_log_path()?)
    }

    fn new(root: PathBuf, log_path: PathBuf) -> Result<Self> {
        let counter_path = root.join("queue.json");
        let state_path = root.join("state.json");
        Ok(Self {
            counter: DailyCounter::load(&counter_path)?,
            state: state::State::load(&state_path)?,
            root,
            log_path,
            counter_path,
            state_path,
            active: None,
        })
    }

    /// Reviewer calls spent today, against `daily_review_budget`. Reported
    /// separately from the judge's count so an operator can see which surface
    /// is spending.
    pub fn calls_today(&self) -> u32 {
        self.counter.count_today()
    }

    /// Runs at most one step of the review and never waits for a model process.
    pub fn tick(&mut self, config: &Config, ledger: &Ledger, now: DateTime<Utc>) -> Result<()> {
        if self.active.is_some() {
            return self.poll_session();
        }
        if !self.due(config, now) {
            return Ok(());
        }
        self.review(config, ledger, now)
    }

    fn due(&self, config: &Config, now: DateTime<Utc>) -> bool {
        let Some(last) = self.state.last_run else {
            return true;
        };
        let interval = i64::try_from(config.overseer.review_interval_mins).unwrap_or(i64::MAX);
        (now - last).num_minutes() >= interval
    }

    fn review(&mut self, config: &Config, ledger: &Ledger, now: DateTime<Utc>) -> Result<()> {
        let decisions = logging::tail_from(&self.log_path, digest::MAX_DECISIONS)?;
        let digest = digest::build(&decisions, ledger, &config.overseer, now);
        let found = findings::detect(&digest, &config.overseer);
        self.state.last_run = Some(now);
        let fresh = state::State::newly_seen(
            &mut self.state.outstanding,
            found.iter().map(|finding| finding.key.clone()).collect(),
        );
        for finding in found.iter().filter(|finding| fresh.contains(&finding.key)) {
            self.log(DecisionKind::Escalate, &finding.reason)?;
        }
        // Saved after the escalations, not before: a crash in between should
        // replay a finding rather than mark it reported and lose it.
        self.state.save(&self.state_path)?;
        // Everything above is arithmetic over the board's own history and costs
        // nothing to run. `review_profile` switches on the model stage below it,
        // not the detection: gating the whole pass on a profile is what left the
        // rules that would have caught a seven-hour hold having never run once.
        if config.overseer.review_profile.is_none() {
            return Ok(());
        }
        if self.counter.count_today() >= config.overseer.daily_review_budget {
            // The deterministic findings above still ran; only the model stage
            // is out of budget, and saying so keeps a quiet reviewer from
            // reading as a healthy board.
            return self.log(DecisionKind::Hold, "review_budget_exhausted");
        }
        self.counter.increment(&self.counter_path)?;
        self.active = Some(spawn_session(config, &digest, &found, &self.root));
        Ok(())
    }

    fn poll_session(&mut self) -> Result<()> {
        let received = match self
            .active
            .as_ref()
            .expect("active review session")
            .try_recv()
        {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => {
                SessionResult::LaunchFailed("review session thread disconnected".into())
            }
        };
        self.active = None;
        match received {
            SessionResult::Result(raw) => match result::parse(&raw) {
                Ok(review) => self.record(review),
                Err(error) => self.log(
                    DecisionKind::Hold,
                    &format!("review result rejected: {error}"),
                ),
            },
            other => self.log(DecisionKind::Hold, &format!("review {}", failed(other))),
        }
    }

    fn record(&mut self, review: result::Review) -> Result<()> {
        self.log(
            DecisionKind::Hold,
            &format!("review summary: {}", review.summary),
        )?;
        let keyed = review
            .findings
            .iter()
            .map(|finding| {
                (
                    format!("{}:{}", finding.severity.label(), finding.summary),
                    finding.severity,
                )
            })
            .collect::<Vec<_>>();
        let fresh = state::State::newly_seen(
            &mut self.state.reported,
            keyed.iter().map(|(key, _)| key.clone()).collect(),
        );
        for (key, severity) in keyed.iter().filter(|(key, _)| fresh.contains(key)) {
            let kind = if severity.escalates() {
                DecisionKind::Escalate
            } else {
                DecisionKind::Hold
            };
            self.log(kind, &format!("review {key}"))?;
        }
        self.state.save(&self.state_path)
    }

    fn log(&self, kind: DecisionKind, reason: &str) -> Result<()> {
        let mut entry = DecisionEntry::new(kind, reason);
        entry.source = Some(SOURCE.into());
        logging::append_to(&self.log_path, &entry)
    }
}

fn spawn_session(
    config: &Config,
    digest: &Digest,
    findings: &[Finding],
    root: &Path,
) -> SessionHandle {
    let profile = review_profile(config);
    let timeout = Duration::from_secs(config.overseer.triage_timeout_mins.saturating_mul(60));
    let case_dir = root.join("session");
    let case = match serde_json::to_vec_pretty(digest) {
        Ok(raw) => raw,
        Err(error) => {
            return SessionHandle::spawn(move |_| SessionResult::LaunchFailed(error.to_string()));
        }
    };
    let prompt = briefing::render(digest, findings, config.language.as_deref());
    SessionHandle::spawn(move |control| {
        run_session(profile, timeout, &case, &prompt, &case_dir, &control)
    })
}

fn run_session(
    profile: Option<Profile>,
    timeout: Duration,
    case: &[u8],
    prompt: &str,
    case_dir: &Path,
    control: &SessionControl,
) -> SessionResult {
    if let Err(error) = fs::create_dir_all(case_dir) {
        return SessionResult::LaunchFailed(error.to_string());
    }
    if let Err(error) = fs::write(case_dir.join("digest.json"), case)
        .and_then(|()| fs::write(case_dir.join("briefing.md"), prompt))
    {
        return SessionResult::LaunchFailed(error.to_string());
    }
    let Some(profile) = profile else {
        return SessionResult::LaunchFailed("review profile not found".into());
    };
    let pid_path = case_dir.join("session.pid");
    terminate_stale_session(&pid_path);
    EphemeralSession {
        profile: &profile,
        case_dir,
        timeout,
    }
    .run_controlled(&result::is_complete, control, Some(&pid_path))
}

pub(crate) fn review_profile(config: &Config) -> Option<Profile> {
    config
        .overseer
        .review_profile
        .as_ref()
        .and_then(|name| session_profile(config, Some(name)))
}

fn failed(result: SessionResult) -> String {
    match result {
        SessionResult::TimedOut => "session timed out".into(),
        SessionResult::Missing => "session exited without result.json".into(),
        SessionResult::LaunchFailed(error) => format!("session failed: {error}"),
        SessionResult::Result(_) => unreachable!("a result is not a failure"),
    }
}

#[cfg(test)]
#[path = "review/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "review/pass_tests.rs"]
mod pass_tests;
