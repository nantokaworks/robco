use super::*;
use crate::config::Config;

#[test]
fn naming_slug_caps_at_word_boundary() {
    assert_eq!(
        cap_name_slug("task-145-this-is-a-very-long-task-title"),
        "task-145-this-is-a-very-long"
    );
}

#[test]
fn naming_slug_keeps_the_number_and_source_when_capping() {
    // The number is what must survive the cap. Capping cuts at the last hyphen
    // inside the budget, and the leading `<n>-dropr-` sits well inside it.
    assert_eq!(
        cap_name_slug("297-dropr-Lead-auto-created-worktree-branch-and-session-names"),
        "297-dropr-Lead-auto-created"
    );
    // A title carrying no hyphen of its own still cannot eat the prefix: the
    // hyphen after the source is the boundary the cap falls back to.
    assert_eq!(
        cap_name_slug("297-dropr-Leadautocreatedworktreebranchnames"),
        "297-dropr"
    );
}

#[test]
fn naming_slug_trims_trailing_hyphen() {
    assert_eq!(cap_name_slug("short-title-"), "short-title");
}

#[test]
fn naming_slug_leaves_short_value_unchanged() {
    assert_eq!(cap_name_slug("short-title"), "short-title");
}

#[test]
fn naming_slug_hard_truncates_without_hyphens() {
    assert_eq!(
        cap_name_slug("abcdefghijklmnopqrstuvwxyz0123456789"),
        "abcdefghijklmnopqrstuvwxyz012345"
    );
}

#[test]
fn naming_slug_leaves_exactly_32_characters_unchanged() {
    let slug = "abcdefghijklmnopqrstuvwxyz012345";
    assert_eq!(slug.chars().count(), 32);
    assert_eq!(cap_name_slug(slug), slug);
}

#[test]
fn naming_slug_caps_unicode_by_character_without_panicking() {
    let slug = "é".repeat(40);
    assert_eq!(cap_name_slug(&slug), "é".repeat(32));
}

#[test]
fn naming_slug_falls_back_for_empty_input() {
    assert_eq!(naming_slug("", None), "agent");
}

#[test]
fn naming_slug_falls_back_when_title_sanitizes_to_empty() {
    assert_eq!(naming_slug("✨", None), "agent");
}

#[test]
fn naming_slug_falls_back_when_explicit_slug_is_only_hyphens() {
    assert_eq!(naming_slug("ignored title", Some("---")), "agent");
}

#[test]
fn naming_slug_sanitizes_explicit_value() {
    assert_eq!(
        naming_slug("ignored title", Some("task 145/explicit")),
        "task-145-explicit"
    );
}

#[test]
fn naming_slug_prefers_explicit_value() {
    assert_eq!(
        naming_slug("ignored title", Some("task-145-explicit-name")),
        "task-145-explicit-name"
    );
}

#[test]
fn branch_prefix_defaults_to_repo_name() {
    let config = Config::default();
    assert_eq!(resolve_branch_prefix(&config, "myapp"), "myapp/");
}

#[test]
fn branch_prefix_uses_explicit_override() {
    let config = Config {
        branch_prefix: Some("robco/".to_string()),
        ..Config::default()
    };
    assert_eq!(resolve_branch_prefix(&config, "myapp"), "robco/");
}

#[test]
fn branch_prefix_sanitizes_repo_name() {
    let config = Config::default();
    assert_eq!(resolve_branch_prefix(&config, "my.repo"), "my-repo/");
}

#[test]
fn branch_prefix_falls_back_when_repo_name_sanitizes_to_empty() {
    let config = Config::default();
    assert_eq!(resolve_branch_prefix(&config, "..."), "robco/");
}

#[test]
fn worker_branch_name_matches_the_prefix_and_slug_it_is_built_from() {
    // The formula `crate::spawn::branch_conflict` checks with must never drift
    // from the one `create_agent_with_launch` actually spawns with — this pins
    // the two together.
    let config = Config::default();
    assert_eq!(
        worker_branch_name(
            &config,
            "myapp",
            "Add a thing",
            Some("42-dropr-Add-a-thing")
        ),
        format!(
            "{}{}",
            resolve_branch_prefix(&config, "myapp"),
            naming_slug("Add a thing", Some("42-dropr-Add-a-thing"))
        )
    );
}
