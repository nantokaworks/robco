use crate::overseer::ledger::Ledger;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LedgerRequest {
    Skip { task: String, user_id: String },
    Retry { task: String, user_id: String },
}

impl LedgerRequest {
    pub(crate) fn attribution(&self) -> (&str, &str) {
        match self {
            Self::Skip { task, user_id } | Self::Retry { task, user_id } => (task, user_id),
        }
    }
}

pub(crate) fn apply(ledger: &mut Ledger, request: LedgerRequest) -> Result<(), String> {
    match request {
        LedgerRequest::Skip { task, .. } => {
            ledger.skip_list.retain(|id| id != &task);
            ledger.skip_list.push(task);
            Ok(())
        }
        LedgerRequest::Retry { task, .. } => {
            let aliases: Vec<_> = ledger
                .entries
                .iter()
                .filter(|entry| entry.task_id == task || entry.display_id == task)
                .flat_map(|entry| [&entry.task_id, &entry.display_id])
                .cloned()
                .collect();
            let mut found = false;
            for entry in ledger
                .entries
                .iter_mut()
                .filter(|entry| entry.task_id == task || entry.display_id == task)
            {
                entry.retries = 0;
                found = true;
            }
            if !found {
                return Err(format!("task not found: {task}"));
            }
            ledger
                .skip_list
                .retain(|id| id != &task && !aliases.contains(id));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_only_mutate_daemon_owned_ledger() {
        let mut ledger = Ledger::default();
        apply(
            &mut ledger,
            LedgerRequest::Skip {
                task: "task-1".into(),
                user_id: "user-1".into(),
            },
        )
        .unwrap();
        assert_eq!(ledger.skip_list, ["task-1"]);
        assert!(
            apply(
                &mut ledger,
                LedgerRequest::Retry {
                    task: "missing".into(),
                    user_id: "user-1".into(),
                }
            )
            .is_err()
        );
    }
}
