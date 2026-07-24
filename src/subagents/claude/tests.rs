use std::{
    fs::{self, File, FileTimes},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use tempfile::TempDir;

use super::*;

struct Fixture {
    _temp: TempDir,
    base_dir: PathBuf,
    worktree: PathBuf,
    project_dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base_dir = temp.path().join(".claude");
        let worktree = PathBuf::from("/Users/x/proj");
        let project_dir = base_dir.join("projects").join(project_slug(&worktree));
        fs::create_dir_all(&project_dir).unwrap();
        Self {
            _temp: temp,
            base_dir,
            worktree,
            project_dir,
        }
    }

    fn session(&self, id: &str) -> PathBuf {
        fs::write(self.project_dir.join(format!("{id}.jsonl")), "session\n").unwrap();
        let path = self.project_dir.join(id).join("subagents");
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn set_session_mtime(&self, id: &str, modified: SystemTime) {
        set_mtime(&self.project_dir.join(format!("{id}.jsonl")), modified);
    }

    fn subagent(&self, dir: &Path, id: &str, json: &str) -> SystemTime {
        fs::write(dir.join(format!("agent-{id}.meta.json")), json).unwrap();
        let activity = dir.join(format!("agent-{id}.jsonl"));
        fs::write(&activity, "activity\n").unwrap();
        fs::metadata(activity).unwrap().modified().unwrap()
    }

    fn read_at(&self, session_id: Option<&str>, now: SystemTime) -> Vec<TaskSubagent> {
        ClaudeSubagentReader::new(&self.base_dir).read(&self.worktree, session_id, now)
    }
}

fn set_mtime(path: &Path, modified: SystemTime) {
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
}

const META: &str = r#"{
    "agentType":"Explore",
    "description":"Find the source",
    "toolUseId":"tool-1",
    "spawnDepth":2
}"#;

#[test]
fn maps_worktree_path_to_claude_project_slug() {
    assert_eq!(project_slug(Path::new("/Users/x/proj")), "-Users-x-proj");
    assert_eq!(project_slug(Path::new("/tmp/a.b c")), "-tmp-a-b-c");
    assert_eq!(project_slug(Path::new("日本語")), "---");
}

#[test]
fn explicit_session_reads_only_its_own_subagents() {
    let fixture = Fixture::new();
    let session_a = fixture.session("session-a");
    let modified = fixture.subagent(&session_a, "agent-a", META);
    let session_b = fixture.session("session-b");
    fixture.subagent(&session_b, "agent-b", META);

    let agents = fixture.read_at(Some("session-a"), modified + Duration::from_secs(1));
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, "agent-a");
}

#[test]
fn explicit_session_does_not_fall_back_when_its_subagents_are_missing() {
    let fixture = Fixture::new();
    fixture.session("session-a");
    let session_b = fixture.session("session-b");
    let modified = fixture.subagent(&session_b, "agent-b", META);

    assert!(
        fixture
            .read_at(Some("session-a"), modified + Duration::from_secs(1))
            .is_empty()
    );
}

#[test]
fn implicit_session_reads_one_recent_session_but_rejects_ambiguity() {
    let fixture = Fixture::new();
    let session_a = fixture.session("session-a");
    let modified = fixture.subagent(&session_a, "agent-a", META);

    let agents = fixture.read_at(None, modified + Duration::from_secs(1));
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, "agent-a");

    let session_b = fixture.session("session-b");
    fixture.subagent(&session_b, "agent-b", META);
    assert!(
        fixture
            .read_at(None, modified + Duration::from_secs(1))
            .is_empty()
    );
}

#[test]
fn classifies_recent_activity_as_running_and_older_activity_as_done() {
    let fixture = Fixture::new();
    let session = fixture.session("session");
    let modified = fixture.subagent(&session, "worker", META);

    assert_eq!(
        fixture
            .read_at(Some("session"), modified + RUNNING_RECENCY)
            .first()
            .unwrap()
            .status,
        SubagentStatus::Running
    );
    assert_eq!(
        fixture
            .read_at(
                Some("session"),
                modified + RUNNING_RECENCY + Duration::from_secs(1),
            )
            .first()
            .unwrap()
            .status,
        SubagentStatus::Done
    );
}

#[test]
fn hides_done_subagents_outside_recency_window() {
    let fixture = Fixture::new();
    let session = fixture.session("session");
    let modified = fixture.subagent(&session, "worker", META);
    let now = modified + DONE_RECENCY + Duration::from_secs(1);
    fixture.set_session_mtime("session", now);

    assert!(fixture.read_at(Some("session"), now).is_empty());
}

#[test]
fn rejects_mtimes_beyond_future_skew_tolerance() {
    let fixture = Fixture::new();
    let valid = fixture.session("valid");
    let modified = fixture.subagent(&valid, "valid-agent", META);
    let future_session = fixture.session("future-session");
    fixture.subagent(&future_session, "future-session-agent", META);
    let now = modified + Duration::from_secs(1);
    fixture.set_session_mtime("valid", now);
    fixture.set_session_mtime(
        "future-session",
        now + FUTURE_MTIME_TOLERANCE + Duration::from_secs(1),
    );

    let agents = fixture.read_at(Some("valid"), now);
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, "valid-agent");

    set_mtime(
        &valid.join("agent-valid-agent.jsonl"),
        now + FUTURE_MTIME_TOLERANCE + Duration::from_secs(1),
    );
    assert!(fixture.read_at(Some("valid"), now).is_empty());
}

#[test]
fn skips_malformed_metadata_but_returns_valid_entries() {
    let fixture = Fixture::new();
    let session = fixture.session("session");
    fixture.subagent(&session, "broken", "{not-json");
    let modified = fixture.subagent(&session, "valid", META);

    let agents = fixture.read_at(Some("session"), modified + Duration::from_secs(1));
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, "valid");
    assert_eq!(agents[0].agent_type, "Explore");
    assert_eq!(agents[0].description, "Find the source");
    assert_eq!(agents[0].spawn_depth, 2);
}

#[test]
fn missing_project_or_subagents_directory_returns_empty() {
    let temp = tempfile::tempdir().unwrap();
    let reader = ClaudeSubagentReader::new(temp.path());
    assert!(
        reader
            .read(Path::new("/missing"), None, SystemTime::now())
            .is_empty()
    );

    let fixture = Fixture::new();
    fs::write(fixture.project_dir.join("session.jsonl"), "session\n").unwrap();
    assert!(fixture.read_at(None, SystemTime::now()).is_empty());
}
