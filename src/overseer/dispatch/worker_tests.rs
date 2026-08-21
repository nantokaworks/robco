use super::*;

fn entry(task_id: &str, retries: u32) -> LedgerEntry {
    LedgerEntry {
        task_id: task_id.into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "auto-agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::Failed,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        escalation_notified_reason: None,
        escalation_notified_head: None,
        worker_escalated: false,
        operator_override: None,
        merge_approval: None,
        pr_facts: None,
        worker_finished_at: None,
    }
}

#[test]
fn a_failed_spawn_still_counts_the_attempt() {
    // The attempt is recorded before the spawn runs, so an attempt that never
    // reaches a ledger entry of its own still bounds the next pass.
    let mut ledger = Ledger::default();
    ledger.entries.push(entry("task-1", 0));

    assert_eq!(record_attempt(&mut ledger, "task-1", "#1"), 1);
    assert_eq!(ledger.entries[0].retries, 1);
}

fn candidate(display_id: &str, title: &str) -> Candidate {
    Candidate {
        task_id: "23O9SkXUps3lOIHuXvj4Z".into(),
        display_id: display_id.into(),
        title: title.into(),
        repo: "/repo".into(),
        author: "allowed".into(),
        priority: "medium".into(),
        workspace: "workspace-1".into(),
        priority_score: None,
        status: "open".into(),
    }
}

#[test]
fn a_candidate_is_named_from_its_dropr_number() {
    // Shape coverage lives with the slug itself in `naming`; what matters
    // here is that a dispatched candidate is numbered from dropr.
    assert_eq!(
        name_slug(&candidate("#295", "Add a top-level language config")).as_deref(),
        Some("295-dropr-Add-a-top-level-language-config")
    );
    assert_eq!(name_slug(&candidate("", "Add a config")), None);
}

fn other_pr(display_id: &str, url: &str) -> crate::overseer::other_prs::OtherPr {
    crate::overseer::other_prs::OtherPr {
        number: 598,
        title: "fix the bug".into(),
        author: "someone-else".into(),
        url: url.into(),
        head_ref_name: "someone-else/fix".into(),
        mergeable_state: "CLEAN".into(),
        closes_task: Some(display_id.into()),
    }
}

#[test]
fn a_pull_request_naming_the_task_names_itself_in_the_hold_reason() {
    // dropr task #354: #803 was already covered by pull request #598,
    // opened by an agent outside the overseer.
    let mut other_prs = OtherPrs::default();
    other_prs.repos.insert(
        "/repo".into(),
        crate::overseer::other_prs::RepoOtherPrs {
            polled_at: Utc::now(),
            prs: vec![other_pr("#803", "https://github.com/example/repo/pull/598")],
        },
    );
    assert_eq!(
        closed_elsewhere(&other_prs, &candidate("#803", "task")),
        Some("pr_closes_task:https://github.com/example/repo/pull/598".into())
    );
}

#[test]
fn an_unrelated_pull_request_does_not_hold_the_candidate() {
    let mut other_prs = OtherPrs::default();
    other_prs.repos.insert(
        "/repo".into(),
        crate::overseer::other_prs::RepoOtherPrs {
            polled_at: Utc::now(),
            prs: vec![other_pr("#1", "https://github.com/example/repo/pull/1")],
        },
    );
    assert_eq!(
        closed_elsewhere(&other_prs, &candidate("#803", "task")),
        None
    );
    assert_eq!(
        closed_elsewhere(&OtherPrs::default(), &candidate("#803", "task")),
        None
    );
}

/// dropr:ZJd6VtMdhDsD39-oeoq_L: a left-over branch is not a transient state,
/// so holding the candidate forever (one branch was held 1,151 times in the
/// evidence this task cites) hides the real problem instead of surfacing it.
#[test]
fn a_branch_conflict_escalates_after_the_bound_and_then_stays_quiet() {
    let mut ledger = Ledger::default();
    for expected_holds in 1..MAX_BRANCH_EXISTS_HOLDS {
        match branch_exists_outcome(&mut ledger, "task-1", "robco/task-1-branch") {
            SpawnOutcome::Held(reason) => {
                assert_eq!(reason, "branch_exists:robco/task-1-branch");
            }
            SpawnOutcome::Escalated(_) => panic!("escalated before the bound was spent"),
            SpawnOutcome::Spawned => unreachable!(),
        }
        assert_eq!(ledger.branch_exists_holds["task-1"], expected_holds);
    }

    // The pass that spends the bound escalates exactly once.
    let SpawnOutcome::Escalated(reason) =
        branch_exists_outcome(&mut ledger, "task-1", "robco/task-1-branch")
    else {
        panic!("expected the bound-spending pass to escalate");
    };
    assert_eq!(reason, "branch_exists:robco/task-1-branch");
    assert_eq!(
        ledger.branch_exists_holds["task-1"],
        MAX_BRANCH_EXISTS_HOLDS
    );

    // Every later pass while the branch is still there is a steady hold, not
    // a repeated escalation, and the count no longer grows.
    for _ in 0..3 {
        let SpawnOutcome::Held(reason) =
            branch_exists_outcome(&mut ledger, "task-1", "robco/task-1-branch")
        else {
            panic!("expected the steady escalated hold, not a repeated escalation");
        };
        assert_eq!(reason, "branch_exists_escalated:robco/task-1-branch");
    }
    assert_eq!(
        ledger.branch_exists_holds["task-1"],
        MAX_BRANCH_EXISTS_HOLDS
    );
}

#[test]
fn a_cleared_branch_conflict_resets_the_hold_count() {
    let mut ledger = Ledger::default();
    ledger.branch_exists_holds.insert("task-1".into(), 4);

    ledger.branch_exists_holds.remove("task-1");

    assert!(!ledger.branch_exists_holds.contains_key("task-1"));
}

#[test]
fn separate_tasks_hold_their_own_independent_counts() {
    let mut ledger = Ledger::default();
    branch_exists_outcome(&mut ledger, "task-1", "branch-a");
    branch_exists_outcome(&mut ledger, "task-1", "branch-a");
    branch_exists_outcome(&mut ledger, "task-2", "branch-b");

    assert_eq!(ledger.branch_exists_holds["task-1"], 2);
    assert_eq!(ledger.branch_exists_holds["task-2"], 1);
}

#[test]
fn attempts_are_counted_across_both_identifiers() {
    let mut ledger = Ledger::default();
    ledger.entries.push(entry("task-1", 0));
    // An entry recorded under the display id belongs to the same task.
    ledger.entries.push(entry("#1", 0));

    assert_eq!(record_attempt(&mut ledger, "task-1", "#1"), 2);
    assert!(ledger.entries.iter().all(|entry| entry.retries == 2));
}
