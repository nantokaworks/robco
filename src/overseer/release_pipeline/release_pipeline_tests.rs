use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

use super::*;
use crate::git::test_repo::{TestRepo, git};

fn cargo_toml(name: &str, version: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n")
}

fn output(code: i32, stdout: &str, stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(code << 8),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn is_self_true_only_for_this_projects_own_package_name() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        cargo_toml(env!("CARGO_PKG_NAME"), "0.1.0"),
    )
    .unwrap();
    assert!(is_self(temp.path()));
}

#[test]
fn is_self_false_for_a_different_project() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        cargo_toml("some-other-project", "0.1.0"),
    )
    .unwrap();
    assert!(!is_self(temp.path()));
}

#[test]
fn is_self_false_without_a_cargo_toml() {
    let temp = tempfile::tempdir().unwrap();
    assert!(!is_self(temp.path()));
}

#[test]
fn crate_version_reads_the_packages_own_version() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        cargo_toml(env!("CARGO_PKG_NAME"), "0.1.93"),
    )
    .unwrap();
    assert_eq!(crate_version(temp.path()).as_deref(), Some("0.1.93"));
}

#[test]
fn crate_version_none_without_a_cargo_toml() {
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(crate_version(temp.path()), None);
}

#[test]
fn failure_excerpt_names_the_last_stage_started_and_the_dying_message() {
    let out = output(
        1,
        "[release] version=0.1.93 tag=v0.1.93 root=/repo\n[release] stage: preflight\n[release] preflight OK\n[release] stage: check\n",
        "[release] error: cargo test failed\n",
    );
    assert_eq!(
        failure_excerpt(&out),
        "stage=check [release] error: cargo test failed"
    );
}

#[test]
fn failure_excerpt_falls_back_to_the_last_stderr_line_without_a_die_message() {
    let out = output(1, "[release] stage: build\n", "some unexpected crash\n");
    assert_eq!(failure_excerpt(&out), "stage=build some unexpected crash");
}

#[test]
fn failure_excerpt_handles_no_stage_and_no_output_at_all() {
    let out = output(1, "", "");
    assert_eq!(failure_excerpt(&out), "stage=unknown no output");
}

#[test]
fn ready_rejects_a_dirty_checkout() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    repo.land_squash("task");
    git(repo.path(), &["checkout", "-q", "main"]);

    std::fs::write(repo.path().join("wip.txt"), "not yet committed").unwrap();

    assert_eq!(ready(repo.path()), Err("working_tree_dirty".to_string()));
}

#[test]
fn ready_rejects_a_checkout_behind_the_merged_commit() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    repo.land_squash("task");
    // The primary checkout never moved off the commit it had before the
    // squash landed on `origin/main` — nothing in this module pulls it.
    git(repo.path(), &["checkout", "-q", "main"]);

    assert_eq!(
        ready(repo.path()),
        Err("checkout_not_on_merged_commit".to_string())
    );
}

/// dropr:429 — a detached checkout must be named, not folded into the
/// generic "not on the merged commit" reason.
#[test]
fn ready_rejects_a_detached_checkout() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    repo.land_squash("task");
    git(repo.path(), &["checkout", "-q", "--detach", "main"]);

    assert_eq!(ready(repo.path()), Err("checkout_detached".to_string()));
}

/// dropr:429 — a checkout left on some other branch gets its own reason too.
/// dropr:444 — and that reason carries the branch name, so the
/// operator-facing message can name it too.
#[test]
fn ready_rejects_a_checkout_on_another_branch() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    repo.land_squash("task");
    git(repo.path(), &["checkout", "-qb", "operator-wip"]);

    assert_eq!(
        ready(repo.path()),
        Err("checkout_not_on_main:operator-wip".to_string())
    );
    // `ready` only ever reads; the checkout it found must be exactly the one
    // it leaves behind.
    assert_eq!(
        crate::git::current_branch(repo.path()).unwrap().as_deref(),
        Some("operator-wip")
    );
}

#[test]
fn ready_accepts_a_clean_checkout_already_on_the_merged_commit() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    repo.land_squash("task");
    git(repo.path(), &["checkout", "-q", "main"]);
    git(repo.path(), &["fetch", "-q", "origin"]);
    git(repo.path(), &["merge", "-q", "--ff-only", "origin/main"]);

    assert_eq!(ready(repo.path()), Ok(()));
}

#[test]
fn consider_is_silent_for_a_repository_without_the_release_script() {
    let repo = TestRepo::new();
    // No `scripts/release.sh` in this checkout, so `consider` must return
    // before it ever reads the pull request — which would otherwise shell
    // out to `gh` and hang in a test environment with no such PR.
    consider(
        "task-1",
        &repo.path().display().to_string(),
        Some("https://example.invalid/pr/1"),
        true,
    )
    .unwrap();
}

#[test]
fn consider_is_silent_for_a_repository_that_is_not_this_project() {
    let repo = TestRepo::new();
    std::fs::create_dir_all(repo.path().join("scripts")).unwrap();
    std::fs::write(
        repo.path().join("scripts/release.sh"),
        "#!/usr/bin/env bash\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("Cargo.toml"),
        cargo_toml("some-other-project", "0.1.0"),
    )
    .unwrap();
    consider(
        "task-1",
        &repo.path().display().to_string(),
        Some("https://example.invalid/pr/1"),
        true,
    )
    .unwrap();
}

#[test]
fn consider_is_silent_without_a_pull_request_url() {
    let repo = TestRepo::new();
    std::fs::create_dir_all(repo.path().join("scripts")).unwrap();
    std::fs::write(
        repo.path().join("scripts/release.sh"),
        "#!/usr/bin/env bash\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("Cargo.toml"),
        cargo_toml(env!("CARGO_PKG_NAME"), "0.1.0"),
    )
    .unwrap();
    consider("task-1", &repo.path().display().to_string(), None, true).unwrap();
}

/// The precise regression this guard exists for: a repository that would
/// otherwise qualify on every other guard (own project, has the script, a
/// `[release]`-scoped PR) must still do nothing while the operator has not
/// opted in — proven here by disabling it and expecting no crash even
/// though `pr_title` would need `gh` (unreachable) to get past this guard.
#[test]
fn consider_is_silent_when_the_pipeline_is_disabled_even_for_a_qualifying_repository() {
    let repo = TestRepo::new();
    std::fs::create_dir_all(repo.path().join("scripts")).unwrap();
    std::fs::write(
        repo.path().join("scripts/release.sh"),
        "#!/usr/bin/env bash\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("Cargo.toml"),
        cargo_toml(env!("CARGO_PKG_NAME"), "0.1.0"),
    )
    .unwrap();
    consider(
        "task-1",
        &repo.path().display().to_string(),
        Some("https://example.invalid/pr/1"),
        false,
    )
    .unwrap();
}
