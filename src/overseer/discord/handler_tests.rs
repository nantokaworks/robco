use super::*;

#[derive(Default)]
struct FakeExecutor {
    dispatch: bool,
    worker_stop_requested: bool,
    calls: Vec<(Command, String)>,
    audits: Vec<(String, String)>,
}

impl CommandExecutor for FakeExecutor {
    fn execute(&mut self, command: &Command, user_id: &str) -> Result<String, String> {
        self.calls.push((command.clone(), user_id.into()));
        self.audits.push(("discord".into(), user_id.into()));
        match command {
            Command::Dispatch(enabled) => self.dispatch = *enabled,
            Command::AutoMerge(true) => {
                return Err("refused: automerge can only be enabled via the local CLI".into());
            }
            Command::Panic => {
                self.dispatch = false;
                self.worker_stop_requested = true;
            }
            _ => {}
        }
        Ok("ok".into())
    }
}

fn handler(limit: usize, ttl: u64) -> Handler {
    Handler::new("10".into(), vec!["20".into()], ttl, limit)
}

#[test]
fn non_allowlisted_user_and_channel_are_silent() {
    let mut handler = handler(10, 60);
    let mut executor = FakeExecutor::default();
    assert_eq!(
        handler.handle("wrong", "20", "!status", 0, &mut executor),
        None
    );
    assert_eq!(
        handler.handle("10", "wrong", "!status", 0, &mut executor),
        None
    );
    assert!(executor.calls.is_empty());
}

#[test]
fn kill_requires_matching_unexpired_confirmation() {
    let mut handler = handler(10, 5);
    let mut executor = FakeExecutor::default();
    let prompt = handler
        .handle("10", "20", "!kill worker", 10, &mut executor)
        .unwrap();
    let code = confirmation_code(&prompt);
    assert_eq!(
        handler.handle("10", "20", "CONFIRM wrong", 11, &mut executor),
        Some("confirmation rejected".into())
    );
    assert!(executor.calls.is_empty());
    assert_eq!(
        handler.handle("10", "20", &format!("CONFIRM {code}"), 11, &mut executor),
        Some("ok".into())
    );
    assert!(matches!(executor.calls[0].0, Command::Kill(_)));

    let prompt = handler
        .handle("10", "20", "!kill worker", 20, &mut executor)
        .unwrap();
    let code = confirmation_code(&prompt);
    assert_eq!(
        handler.handle("10", "20", &format!("CONFIRM {code}"), 26, &mut executor),
        Some("confirmation rejected".into())
    );
    assert_eq!(executor.calls.len(), 1);
}

#[test]
fn automerge_on_is_refused_without_execution() {
    let mut handler = handler(10, 60);
    let mut executor = FakeExecutor::default();
    let response = handler.handle("10", "20", "!automerge on", 0, &mut executor);
    assert!(response.unwrap().starts_with("error: refused:"));
    assert_eq!(executor.calls.len(), 1);
}

#[test]
fn action_rate_limit_blocks_after_threshold() {
    let mut handler = handler(2, 60);
    let mut executor = FakeExecutor::default();
    assert_eq!(
        handler.handle("10", "20", "!dispatch off", 1, &mut executor),
        Some("ok".into())
    );
    assert_eq!(
        handler.handle("10", "20", "!automerge off", 2, &mut executor),
        Some("ok".into())
    );
    assert_eq!(
        handler.handle("10", "20", "!dispatch off", 3, &mut executor),
        Some("rate limit exceeded; try again later".into())
    );
    assert_eq!(executor.calls.len(), 2);
}

#[test]
fn panic_disables_dispatch_requests_stop_and_audits_identity() {
    let mut handler = handler(10, 60);
    let mut executor = FakeExecutor {
        dispatch: true,
        ..FakeExecutor::default()
    };
    let prompt = handler
        .handle("10", "20", "!panic", 1, &mut executor)
        .unwrap();
    let code = confirmation_code(&prompt);
    handler.handle("10", "20", &format!("CONFIRM {code}"), 2, &mut executor);
    assert!(!executor.dispatch);
    assert!(executor.worker_stop_requested);
    assert_eq!(executor.audits, [("discord".into(), "20".into())]);
}

fn confirmation_code(prompt: &str) -> &str {
    prompt
        .split("reply `CONFIRM ")
        .nth(1)
        .and_then(|value| value.strip_suffix('`'))
        .unwrap()
}

#[test]
fn dispatch_off_is_visible_to_subsequent_config_read() {
    let mut handler = handler(10, 60);
    let mut executor = FakeExecutor {
        dispatch: true,
        ..FakeExecutor::default()
    };
    handler.handle("10", "20", "!dispatch off", 1, &mut executor);
    assert!(!executor.dispatch);
}

#[test]
fn dispatch_on_requires_confirmation_but_off_is_immediate() {
    let mut handler = handler(10, 60);
    let mut executor = FakeExecutor::default();
    let prompt = handler
        .handle("10", "20", "!dispatch on", 1, &mut executor)
        .unwrap();
    assert!(prompt.starts_with("Confirm: dispatch on — reply `CONFIRM "));
    assert!(executor.calls.is_empty());
    assert_eq!(
        handler.handle("10", "20", "!dispatch off", 2, &mut executor),
        Some("ok".into())
    );
}

#[test]
fn every_risk_increasing_mutation_requires_confirmation() {
    for message in [
        "!kill worker",
        "!panic",
        "!retry task",
        "!skip task",
        "!approve worker",
        "!answer worker proceed",
        "!dispatch on",
        "!merge task",
    ] {
        let mut handler = handler(10, 60);
        let mut executor = FakeExecutor::default();
        let response = handler.handle("10", "20", message, 1, &mut executor);
        assert!(
            response.unwrap().contains(" — reply `CONFIRM "),
            "{message}"
        );
        assert!(executor.calls.is_empty(), "{message}");
    }
}

#[test]
fn generated_confirmation_describes_the_validated_command() {
    let mut handler = handler(10, 60);
    let mut executor = FakeExecutor::default();
    let prompt = handler.submit_generated(
        "10",
        "20",
        Some("case-1".into()),
        Command::Kill("worker-x".into()),
        1,
        &mut executor,
    );
    assert!(
        prompt
            .response
            .starts_with("Confirm: kill worker-x — reply `CONFIRM ")
    );
    assert!(executor.calls.is_empty());
}

#[test]
fn diff_executes_immediately_without_confirmation() {
    let mut handler = handler(10, 60);
    let mut executor = FakeExecutor::default();
    assert_eq!(
        handler.handle("10", "20", "!diff task", 1, &mut executor),
        Some("ok".into())
    );
    assert!(matches!(executor.calls[0].0, Command::Diff(_)));
}

#[test]
fn config_reload_revokes_user_without_rebuilding_handler() {
    let mut handler = handler(10, 60);
    let config = DiscordConfig {
        channel_id: Some("10".into()),
        allowed_user_ids: vec!["other".into()],
        ..Default::default()
    };
    handler.update_config(&config);
    let mut executor = FakeExecutor::default();
    assert_eq!(
        handler.handle("10", "20", "!status", 1, &mut executor),
        None
    );
}
