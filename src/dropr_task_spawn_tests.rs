use super::*;

#[test]
fn a_bare_number_gets_the_display_id_prefix() {
    assert_eq!(normalize_task_ref("538"), "#538");
}

#[test]
fn an_already_prefixed_id_is_left_alone() {
    assert_eq!(normalize_task_ref("#538"), "#538");
}

#[test]
fn a_nanoid_is_left_alone() {
    assert_eq!(
        normalize_task_ref("V1StGXR8_Z5jdHi6B-myT"),
        "V1StGXR8_Z5jdHi6B-myT"
    );
}

#[test]
fn surrounding_whitespace_is_trimmed() {
    assert_eq!(normalize_task_ref("  538  "), "#538");
}

/// dropr:540's acceptance criterion: an unresolvable id and an already
/// claimed task fail with a message naming which of the two happened.
#[test]
fn a_claim_held_by_a_known_holder_names_them() {
    let err = claimed_error("#538", Some("other-agent".to_string()));
    assert_eq!(
        err.to_string(),
        "dropr task #538 is already claimed by other-agent"
    );
}

#[test]
fn a_claim_held_by_an_unknown_holder_still_says_claimed_not_generic() {
    let err = claimed_error("#538", None);
    assert_eq!(
        err.to_string(),
        "dropr task #538 is already claimed by another agent"
    );
}

#[test]
fn a_not_found_task_names_itself_as_such() {
    let err = Error::DroprTaskNotFound("#99999".to_string());
    assert_eq!(err.to_string(), "dropr task not found: #99999");
}

#[test]
fn a_not_found_message_and_a_claimed_message_never_collide() {
    let not_found = Error::DroprTaskNotFound("#538".to_string()).to_string();
    let claimed = claimed_error("#538", Some("other-agent".to_string())).to_string();
    assert_ne!(not_found, claimed);
}

#[test]
fn other_refusal_reasons_are_named_verbatim() {
    let err = refused_error("#538", "blocked".to_string());
    assert_eq!(err.to_string(), "could not claim dropr task #538: blocked");
}

/// Combining `--dropr-task` with an explicit title, prompt, or name-slug is a
/// hard error, before any repo, workspace, or dropr lookup runs.
#[test]
fn an_explicit_title_alongside_dropr_task_is_rejected() {
    let config = Config::default();
    let result = spawn_dropr_task_in_repo(
        "myapp",
        "#538",
        Some("hand-picked title"),
        None,
        None,
        None,
        &[],
        false,
        &config,
    );
    assert!(matches!(result, Err(Error::DroprTaskSpawnConflict)));
}

#[test]
fn an_explicit_prompt_alongside_dropr_task_is_rejected() {
    let config = Config::default();
    let result = spawn_dropr_task_in_repo(
        "myapp",
        "#538",
        None,
        Some("hand-picked prompt"),
        None,
        None,
        &[],
        false,
        &config,
    );
    assert!(matches!(result, Err(Error::DroprTaskSpawnConflict)));
}

#[test]
fn an_explicit_name_slug_alongside_dropr_task_is_rejected() {
    let config = Config::default();
    let result = spawn_dropr_task_in_repo(
        "myapp",
        "#538",
        None,
        None,
        Some("hand-picked-slug"),
        None,
        &[],
        false,
        &config,
    );
    assert!(matches!(result, Err(Error::DroprTaskSpawnConflict)));
}
