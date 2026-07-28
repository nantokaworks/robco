use super::*;
use std::collections::VecDeque;

struct Immediate(Option<Result<Vec<u8>, String>>);

impl PendingSession for Immediate {
    fn poll(&mut self) -> Option<Result<Vec<u8>, String>> {
        self.0.take()
    }
}

struct Canned {
    results: VecDeque<Vec<u8>>,
    requests: Vec<SessionRequest>,
}

impl Canned {
    fn one(raw: &[u8]) -> Self {
        Self {
            results: VecDeque::from([raw.to_vec()]),
            requests: Vec::new(),
        }
    }
}

impl SessionSpawner for Canned {
    fn spawn(&mut self, request: SessionRequest) -> Result<Box<dyn PendingSession>, String> {
        self.requests.push(request);
        Ok(Box::new(Immediate(Some(Ok(self
            .results
            .pop_front()
            .expect("canned result"))))))
    }
}

fn agent(temp: &tempfile::TempDir) -> OpsAgent {
    OpsAgent::load(
        "10".into(),
        vec!["user".into()],
        temp.path().join("triage"),
        temp.path().join("ops/threads.json"),
    )
    .unwrap()
}

#[test]
fn a_category_member_channel_routes_without_being_the_parent_or_a_thread() {
    let temp = tempfile::tempdir().unwrap();
    let mut ops = agent(&temp);
    let mut spawner = Canned::one(br#"{"reply":"hi","actions":[]}"#);
    assert!(
        ops.route("30", "user", "hello", &mut spawner, false)
            .is_none()
    );
    assert!(
        spawner.requests.is_empty(),
        "non-member channel must not spawn"
    );

    let mut spawner = Canned::one(br#"{"reply":"hi","actions":[]}"#);
    assert!(
        ops.route("30", "user", "hello", &mut spawner, true)
            .is_none()
    );
    assert_eq!(spawner.requests.len(), 1);
}

#[test]
fn two_category_channels_run_sessions_concurrently_up_to_the_cap() {
    let temp = tempfile::tempdir().unwrap();
    let mut ops = agent(&temp);
    ops.update_access("10".into(), vec!["user".into()], 2);
    let mut spawner = Canned {
        results: VecDeque::from([
            br#"{"reply":"a","actions":[]}"#.to_vec(),
            br#"{"reply":"b","actions":[]}"#.to_vec(),
        ]),
        requests: Vec::new(),
    };
    assert!(
        ops.route("30", "user", "hello a", &mut spawner, true)
            .is_none()
    );
    assert!(
        ops.route("31", "user", "hello b", &mut spawner, true)
            .is_none()
    );
    assert_eq!(spawner.requests.len(), 2);
    let replies: Vec<String> = ops
        .poll()
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Post { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert!(replies.contains(&"a".to_string()));
    assert!(replies.contains(&"b".to_string()));
}

#[test]
fn a_third_concurrent_channel_beyond_the_cap_gets_the_busy_message_not_a_dropped_message() {
    let temp = tempfile::tempdir().unwrap();
    let mut ops = agent(&temp);
    ops.update_access("10".into(), vec!["user".into()], 1);
    let mut spawner = Canned::one(br#"{"reply":"a","actions":[]}"#);
    assert!(
        ops.route("30", "user", "hello a", &mut spawner, true)
            .is_none()
    );
    let overflow = ops
        .route("31", "user", "hello b", &mut spawner, true)
        .expect("a second concurrent channel at cap 1 must not be silently dropped");
    assert!(matches!(
        overflow,
        Effect::Post { text, .. } if text.contains("another request")
    ));
}
