use super::*;

/// The request shape both writes are pinned against. A change here means the
/// argument names went out of step with the tool schema, which is the drift
/// that made every Overseer note vanish the last time — see the module doc.
#[test]
fn a_scribble_names_the_task_the_author_and_the_body() {
    assert_eq!(
        scribble_arguments("wTRwfA2poXKH_qJX1IdvT", None, "handed the failure back"),
        json!({
            "items": [{
                "task_id": "wTRwfA2poXKH_qJX1IdvT",
                "agent_id": "overseer",
                "kind": "note",
                "body": "handed the failure back",
            }],
        })
    );
}

/// A `#N` display id needs a `repo_url` to scope the call, or dropr rejects
/// it outright — see `scribble_create_timeout`'s doc comment (dropr:556).
#[test]
fn a_scribble_for_a_display_id_carries_the_repo_url() {
    assert_eq!(
        scribble_arguments(
            "#556",
            Some("https://github.com/nantokaworks/robco.git"),
            "handed the failure back"
        ),
        json!({
            "items": [{
                "task_id": "#556",
                "agent_id": "overseer",
                "kind": "note",
                "body": "handed the failure back",
                "repo_url": "https://github.com/nantokaworks/robco.git",
            }],
        })
    );
}

#[test]
fn a_status_update_names_the_claim_holder() {
    assert_eq!(
        status_arguments("wTRwfA2poXKH_qJX1IdvT", "open"),
        json!({
            "items": [{
                "task_id": "wTRwfA2poXKH_qJX1IdvT",
                "agent_id": "overseer",
                "status": "open",
            }],
        })
    );
}

/// `workspace_id` MUST sit on the bulk envelope, not inside the item. The
/// installed `dropr mcp-stdio` server only honors it there — an item-level
/// `workspace_id` is refused with "exactly one of workspace_id,
/// canonical_repo, repo_url must be set" even though the tool's advertised
/// schema allows either placement (dropr:467). Verified live against a
/// scratch dropr workspace before writing this test.
#[test]
fn a_task_create_names_the_workspace_on_the_envelope() {
    assert_eq!(
        task_create_arguments("Xdin9xDHmhuOohKzCBmZX", "file the follow-up", None),
        json!({
            "workspace_id": "Xdin9xDHmhuOohKzCBmZX",
            "items": [{
                "agent_id": "overseer",
                "title": "file the follow-up",
            }],
        })
    );
    assert_eq!(
        task_create_arguments(
            "Xdin9xDHmhuOohKzCBmZX",
            "file the follow-up",
            Some("more detail"),
        ),
        json!({
            "workspace_id": "Xdin9xDHmhuOohKzCBmZX",
            "items": [{
                "agent_id": "overseer",
                "title": "file the follow-up",
                "description": "more detail",
            }],
        })
    );
}

/// A blank `workspace_id` — the failure mode a bad overlay parse produces —
/// is refused locally rather than round-tripped to `dropr mcp-stdio`, which
/// would otherwise answer with a refusal that reads like a caller bug in
/// dropr rather than the empty value it actually is (dropr task #445).
#[test]
fn a_blank_workspace_id_is_refused_before_the_round_trip() {
    assert_eq!(
        task_create_timeout("", "title", None, Duration::from_millis(50)),
        Err(WriteError::Refused(
            "task_create_timeout: workspace_id is blank".into()
        ))
    );
    assert_eq!(
        task_create_timeout("   ", "title", None, Duration::from_millis(50)),
        Err(WriteError::Refused(
            "task_create_timeout: workspace_id is blank".into()
        ))
    );
}

/// dropr has no `ready` status and rejects the transition outright, so triage's
/// word for "released" has to become dropr's before the call goes out.
#[test]
fn triages_ready_becomes_dropr_open() {
    assert_eq!(lifecycle_status("ready"), "open");
    assert_eq!(lifecycle_status("open"), "open");
    assert_eq!(lifecycle_status("closed"), "closed");
}

/// Captured from `dropr mcp-stdio` 0.4.37: a write that landed.
fn accepted_payload() -> Value {
    json!({
        "next_cursor": null,
        "results": [{
            "id": "xks9xF2zoS3FC0LfDLl2d",
            "status": "ok",
            "payload": { "kind": "note", "task_id": "wTRwfA2poXKH_qJX1IdvT" },
        }],
    })
}

/// Captured from the same server: the item failed inside a call that succeeded.
fn rejected_payload() -> Value {
    json!({
        "next_cursor": null,
        "results": [{
            "id": "wTRwfA2poXKH_qJX1IdvT",
            "status": "error",
            "error": { "code": "-32000", "message": "invalid transition: in_progress -> ready" },
        }],
    })
}

#[test]
fn a_per_item_rejection_is_not_an_accepted_write() {
    assert!(accepted(&accepted_payload()));
    assert!(!accepted(&rejected_payload()));
    // An envelope that answered about nothing wrote nothing.
    assert!(!accepted(&json!({ "results": [] })));
    assert!(!accepted(&json!({ "next_cursor": null })));
}

#[test]
fn a_rejected_item_carries_the_servers_message() {
    assert_eq!(
        verdict(Some(ToolOutcome::Ok(rejected_payload()))),
        Err(WriteError::Refused(
            "invalid transition: in_progress -> ready".into()
        ))
    );
}

/// The distinction the decision log has to keep: a refusal is a fault somebody
/// has to fix, an unavailable dropr is a pass that may succeed next time.
#[test]
fn a_refusal_and_a_missing_answer_are_different_failures() {
    assert_eq!(verdict(Some(ToolOutcome::Ok(accepted_payload()))), Ok(()));
    assert_eq!(
        verdict(Some(ToolOutcome::Refused("Method not found".into()))),
        Err(WriteError::Refused("Method not found".into()))
    );
    assert_eq!(verdict(None), Err(WriteError::Unavailable));

    assert!(
        WriteError::Refused("Method not found".into())
            .to_string()
            .starts_with("refused: ")
    );
    assert!(
        WriteError::Unavailable
            .to_string()
            .starts_with("unavailable: ")
    );
}

#[test]
fn a_server_message_survives_as_one_bounded_line() {
    assert_eq!(one_line("task not found:\n  nope"), "task not found: nope");
    assert_eq!(one_line(&"x".repeat(500)).chars().count(), MESSAGE_LIMIT);
    // A shape carrying no per-item message still has to say something.
    assert!(!item_refusal(&json!({ "results": [{ "status": "error" }] })).is_empty());
}
