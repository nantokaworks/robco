use crate::overseer::logging::DecisionEntry;
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::PathBuf,
};

const STARTUP_BACKLOG: usize = 20;

pub(crate) struct PendingDecision {
    pub entry: Option<DecisionEntry>,
    next_offset: u64,
}

pub(crate) struct DecisionCursor {
    log_path: PathBuf,
    cursor_path: PathBuf,
    offset: u64,
}

impl DecisionCursor {
    pub(crate) fn load(log_path: PathBuf, cursor_path: PathBuf) -> crate::Result<Self> {
        let log_len = fs::metadata(&log_path).map(|meta| meta.len()).unwrap_or(0);
        let offset = match fs::read_to_string(&cursor_path) {
            Ok(raw) => raw.trim().parse::<u64>().unwrap_or(0).min(log_len),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                backlog_start(&log_path, STARTUP_BACKLOG)?
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            log_path,
            cursor_path,
            offset,
        })
    }

    pub(crate) fn next_batch(&self, limit: usize) -> crate::Result<Vec<PendingDecision>> {
        let mut file = match File::open(&self.log_path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        file.seek(SeekFrom::Start(self.offset))?;
        let mut reader = BufReader::new(file);
        let mut pending = Vec::with_capacity(limit);
        let mut offset = self.offset;
        while pending.len() < limit {
            let mut line = String::new();
            let bytes = reader.read_line(&mut line)?;
            if bytes == 0 || !line.ends_with('\n') {
                break;
            }
            offset += bytes as u64;
            pending.push(PendingDecision {
                entry: serde_json::from_str(&line).ok(),
                next_offset: offset,
            });
        }
        Ok(pending)
    }

    pub(crate) fn complete(
        &mut self,
        pending: PendingDecision,
        delivered: bool,
    ) -> crate::Result<bool> {
        if pending.entry.is_some() && !delivered {
            return Ok(false);
        }
        if let Some(parent) = self.cursor_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_path = self.cursor_path.with_extension("cursor.tmp");
        fs::write(&temp_path, pending.next_offset.to_string())?;
        fs::rename(temp_path, &self.cursor_path)?;
        self.offset = pending.next_offset;
        Ok(true)
    }
}

fn backlog_start(path: &PathBuf, limit: usize) -> crate::Result<u64> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let mut reader = BufReader::new(file);
    let mut offsets = VecDeque::with_capacity(limit);
    let mut offset = 0_u64;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        if line.ends_with('\n') && serde_json::from_str::<DecisionEntry>(&line).is_ok() {
            if offsets.len() == limit {
                offsets.pop_front();
            }
            offsets.push_back(offset);
        }
        offset += bytes as u64;
    }
    Ok(offsets.front().copied().unwrap_or(offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::config::DiscordConfig;
    use crate::overseer::discord::notify::{bounded_batch, next_notification};
    use crate::overseer::logging::{self, DecisionEntry, DecisionKind};

    #[test]
    fn cursor_only_advances_a_deliverable_entry_after_delivery() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("decisions.jsonl");
        let cursor_path = temp.path().join("discord.cursor");
        logging::append_to(&log, &DecisionEntry::new(DecisionKind::Hold, "event")).unwrap();
        let mut cursor = DecisionCursor::load(log, cursor_path.clone()).unwrap();
        let pending = cursor.next_batch(1).unwrap().pop().unwrap();
        assert!(!cursor.complete(pending, false).unwrap());
        assert!(!cursor_path.exists());
        let pending = cursor.next_batch(1).unwrap().pop().unwrap();
        assert!(cursor.complete(pending, true).unwrap());
        assert!(cursor_path.exists());
        assert!(cursor.next_batch(1).unwrap().is_empty());
    }

    #[test]
    fn failed_front_entry_keeps_later_escalations_pending_once() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("decisions.jsonl");
        let cursor_path = temp.path().join("discord.cursor");
        // `task_started` rather than `pr_opened`: it fires at the
        // `notify_level` default (`summary`), unlike `pr_opened` (`all`-tier),
        // and this test is exercising cursor retry ordering, not gating.
        let mut first = DecisionEntry::new(DecisionKind::Dispatch, "task_started");
        first.source = Some("daemon_event".into());
        logging::append_to(&log, &first).unwrap();
        for reason in ["one", "two"] {
            logging::append_to(&log, &DecisionEntry::new(DecisionKind::Escalate, reason)).unwrap();
        }
        let mut cursor = DecisionCursor::load(log, cursor_path).unwrap();

        let mut batch = cursor.next_batch(3).unwrap();
        assert_eq!(batch.len(), 3);
        assert!(!cursor.complete(batch.remove(0), false).unwrap());
        let retry = VecDeque::from(cursor.next_batch(3).unwrap());
        assert_eq!(retry.len(), 3);
        assert_eq!(retry[0].entry.as_ref().unwrap().reason, "task_started");
        let (count, notification) = next_notification(&DiscordConfig::default(), &retry);
        assert_eq!(count, 1);
        assert!(notification.is_some());

        let mut retry = retry;
        assert!(cursor.complete(retry.pop_front().unwrap(), true).unwrap());
        let escalations = VecDeque::from(cursor.next_batch(3).unwrap());
        assert_eq!(escalations.len(), 2);
        let (count, notification) = next_notification(&DiscordConfig::default(), &escalations);
        assert_eq!(count, 2);
        assert!(notification.is_some());
        for pending in escalations {
            assert!(cursor.complete(pending, true).unwrap());
        }
        assert!(cursor.next_batch(3).unwrap().is_empty());
    }

    #[test]
    fn escalation_run_beyond_tick_limit_is_one_atomic_digest() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("decisions.jsonl");
        let cursor_path = temp.path().join("discord.cursor");
        for index in 0..25 {
            logging::append_to(
                &log,
                &DecisionEntry::new(DecisionKind::Escalate, format!("event {index}")),
            )
            .unwrap();
        }
        fs::write(&cursor_path, "0").unwrap();
        let mut cursor = DecisionCursor::load(log, cursor_path).unwrap();
        let mut pending = bounded_batch(cursor.next_batch(500).unwrap(), 20);

        assert_eq!(pending.len(), 25);
        let (count, notification) = next_notification(&DiscordConfig::default(), &pending);
        assert_eq!(count, 25);
        assert!(notification.is_some());
        let last = pending.drain(..count).next_back().unwrap();
        assert!(cursor.complete(last, true).unwrap());
        assert!(cursor.next_batch(500).unwrap().is_empty());
    }
}
