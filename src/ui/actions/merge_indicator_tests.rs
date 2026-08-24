//! The merge row indicator's own lifecycle (dropr:545).
//!
//! `Indicator::Merging` animates now, so the row claims robco is running
//! `git` and `gh` this instant. That claim is only honest if it starts when
//! the merge starts and stops on every way the merge can end. What drives it
//! is [`App::is_merging_agent`], so that is what these pin.

use super::*;

const REPO: &str = "/repo";
const AGENT: &str = "wanted";

fn app_with_one_agent() -> App {
    let mut app = test_app();
    app.registry.repos = vec![repo(REPO, vec![agent(AGENT)])];
    app
}

fn merging(app: &App) -> bool {
    app.is_merging_agent(&PathBuf::from(REPO), AGENT)
}

#[test]
fn nothing_animates_before_a_merge_starts() {
    assert!(!merging(&app_with_one_agent()));
}

#[test]
fn a_running_merge_marks_its_own_agent_only() {
    let mut app = app_with_one_agent();
    install_job(&mut app, REPO, AGENT);

    assert!(merging(&app));
    assert!(!app.is_merging_agent(&PathBuf::from(REPO), "someone-else"));
}

#[test]
fn a_merge_that_succeeded_stops_animating() {
    let mut app = app_with_one_agent();
    let (sender, receiver) = mpsc::channel();
    app.merge_jobs.insert(
        REPO.into(),
        MergeJob {
            agent_id: AGENT.into(),
            branch: format!("feature/{AGENT}"),
            step: MERGING_PR,
            receiver,
        },
    );
    sender.send(MergeEvent::Finished(Ok(()))).unwrap();

    app.drain_merge_events().unwrap();

    assert!(!merging(&app));
}

#[test]
fn a_merge_that_failed_stops_animating() {
    let mut app = app_with_one_agent();
    let (sender, receiver) = mpsc::channel();
    app.merge_jobs.insert(
        REPO.into(),
        MergeJob {
            agent_id: AGENT.into(),
            branch: format!("feature/{AGENT}"),
            step: MERGING_PR,
            receiver,
        },
    );
    sender
        .send(MergeEvent::Finished(Err("gh pr merge failed".into())))
        .unwrap();

    app.drain_merge_events().unwrap();

    assert!(!merging(&app));
}

/// The worker thread died without saying anything. The row must not keep
/// spinning on a merge nobody is running.
#[test]
fn a_merge_whose_worker_vanished_stops_animating() {
    let mut app = app_with_one_agent();
    let (sender, receiver) = mpsc::channel();
    app.merge_jobs.insert(
        REPO.into(),
        MergeJob {
            agent_id: AGENT.into(),
            branch: format!("feature/{AGENT}"),
            step: MERGING_PR,
            receiver,
        },
    );
    drop(sender);

    app.drain_merge_events().unwrap();

    assert!(!merging(&app));
}
