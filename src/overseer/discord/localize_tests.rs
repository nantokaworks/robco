use super::*;
use std::{collections::VecDeque, sync::mpsc};

fn notification(title: &str, description: &str) -> Notification {
    Notification {
        title: title.into(),
        description: description.into(),
        color: 0x123456,
        fields: vec![super::super::notifications::NotificationField {
            name: "Task".into(),
            value: "`task-1`".into(),
        }],
    }
}

struct Canned {
    results: VecDeque<Result<Vec<u8>, String>>,
    spawn_calls: usize,
}

impl Canned {
    fn one(result: Result<Vec<u8>, String>) -> Self {
        Self {
            results: VecDeque::from([result]),
            spawn_calls: 0,
        }
    }
}

struct Immediate(Option<Result<Vec<u8>, String>>);
impl PendingSession for Immediate {
    fn poll(&mut self) -> Option<Result<Vec<u8>, String>> {
        self.0.take()
    }
}

impl LocalizeSpawner for Canned {
    fn spawn(
        &mut self,
        _language: &str,
        _notification: &Notification,
    ) -> Result<Box<dyn PendingSession>, String> {
        self.spawn_calls += 1;
        Ok(Box::new(Immediate(Some(
            self.results.pop_front().expect("canned result"),
        ))))
    }
}

#[test]
fn absent_language_never_spawns() {
    let mut spawner = Canned::one(Ok(br#"{"title":"x","description":"y"}"#.to_vec()));
    let cache = TitleCache::default();
    let outcome = start(&mut spawner, &cache, None, notification("Merged", "body"));
    assert!(matches!(outcome, LocalizeOutcome::Ready(_)));
    assert_eq!(spawner.spawn_calls, 0);
}

#[test]
fn successful_localization_replaces_title_and_description() {
    let mut spawner = Canned::one(Ok(r#"{"title":"マージ済み","description":"詳細"}"#
        .as_bytes()
        .to_vec()));
    let cache = TitleCache::default();
    let outcome = start(
        &mut spawner,
        &cache,
        Some("Japanese"),
        notification("Merged", "body"),
    );
    let LocalizeOutcome::Pending(mut session) = outcome else {
        panic!("expected a spawned session");
    };
    let result = session.poll().expect("session already resolved");
    let mut cache = TitleCache::default();
    let localized = resolve(
        &mut cache,
        "Japanese",
        &notification("Merged", "body"),
        result,
    );
    assert_eq!(localized.title, "マージ済み");
    assert_eq!(localized.description, "詳細");
    assert_eq!(localized.color, 0x123456);
    // Fields carry ids and links; they must come through untranslated.
    assert_eq!(localized.fields, notification("Merged", "body").fields);
}

#[test]
fn each_failure_mode_falls_back_to_the_english_notification() {
    let fallback = notification("Merged", "body");
    let mut cache = TitleCache::default();
    for result in [
        Err::<Vec<u8>, String>("session timed out".into()),
        Ok(b"not json".to_vec()),
        Ok(br#"{"title":"","description":""}"#.to_vec()),
    ] {
        let localized = resolve(&mut cache, "Japanese", &fallback, result);
        assert_eq!(localized, fallback);
    }
}

#[test]
fn a_spawn_failure_falls_back_to_english_without_a_pending_session() {
    struct AlwaysFails;
    impl LocalizeSpawner for AlwaysFails {
        fn spawn(
            &mut self,
            _language: &str,
            _notification: &Notification,
        ) -> Result<Box<dyn PendingSession>, String> {
            Err("no profile".into())
        }
    }
    let mut spawner = AlwaysFails;
    let cache = TitleCache::default();
    let fallback = notification("Merged", "body");
    let outcome = start(&mut spawner, &cache, Some("Japanese"), fallback.clone());
    match outcome {
        LocalizeOutcome::Ready(notification) => assert_eq!(notification, fallback),
        LocalizeOutcome::Pending(_) => panic!("a failed spawn must not be pending"),
    }
}

#[test]
fn a_repeated_notification_is_served_from_cache_without_a_second_spawn() {
    let mut spawner = Canned::one(Ok(br#"{"title":"t","description":"d"}"#.to_vec()));
    let mut cache = TitleCache::default();
    let source = notification("Merged", "body");

    let outcome = start(&mut spawner, &cache, Some("Japanese"), source.clone());
    let LocalizeOutcome::Pending(mut session) = outcome else {
        panic!("expected the first call to spawn");
    };
    let result = session.poll().expect("resolved");
    let localized = resolve(&mut cache, "Japanese", &source, result);
    assert_eq!(spawner.spawn_calls, 1);

    let outcome = start(&mut spawner, &cache, Some("Japanese"), source);
    match outcome {
        LocalizeOutcome::Ready(notification) => assert_eq!(notification, localized),
        LocalizeOutcome::Pending(_) => panic!("a cached notification must not spawn again"),
    }
    assert_eq!(
        spawner.spawn_calls, 1,
        "cache hit must not spawn a second session"
    );
}

#[test]
fn briefing_places_the_directive_ahead_of_the_fenced_notification_text() {
    let text = briefing(&notification("Merged", "body"), "Japanese");
    let directive = text.find("LANGUAGE: ").expect("directive is rendered");
    let first_fence = text
        .find("<<<EXTERNAL_DATA ")
        .expect("notification text is fenced");
    assert!(directive < first_fence, "{text}");
    assert!(text.contains("<<<EXTERNAL_DATA NOTIFICATION_TITLE>>>\nMerged"));
    assert!(text.contains("<<<EXTERNAL_DATA NOTIFICATION_BODY>>>\nbody"));
}

#[test]
fn briefing_collection_does_not_block_spawn_caller() {
    let temp = tempfile::tempdir().unwrap();
    let (release, blocked) = mpsc::channel();
    let start = std::time::Instant::now();
    let handle = spawn_session(
        temp.path().join("case"),
        crate::config::Profile {
            name: "test".into(),
            program: "/bin/true".into(),
            autonomous_args: Vec::new(),
            model: None,
            backend: None,
            clear_command: None,
        },
        Duration::from_secs(1),
        SessionEnv::default(),
        move || {
            blocked.recv().unwrap();
            "briefing".into()
        },
    );
    assert!(start.elapsed() < Duration::from_millis(100));
    release.send(()).unwrap();
    drop(handle);
}
