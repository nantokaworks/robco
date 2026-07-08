use std::{fs, str::FromStr};

use serde_json::{Value, json};
use toml_edit::DocumentMut;

use super::{claude, codex, openclaw, read_optional_config};

#[test]
fn optional_config_read_treats_only_not_found_as_absent() {
    let dir = tempfile::tempdir().unwrap();

    assert!(
        read_optional_config(&dir.path().join("missing"))
            .unwrap()
            .is_none()
    );
    assert!(read_optional_config(dir.path()).is_err());
}

#[test]
fn optional_config_read_invalid_utf8_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");

    fs::write(&path, [0xff]).unwrap();

    assert!(read_optional_config(&path).is_err());
}

#[test]
fn claude_upsert_is_idempotent_and_preserves_unrelated_keys() {
    let once = claude::upsert_claude(Some(
        r#"{"theme":"dark","mcpServers":{"other":{"command":"x"}}}"#,
    ))
    .unwrap();
    let twice = claude::upsert_claude(Some(&once)).unwrap();
    let root = json_root(&once);

    assert_eq!(once, twice);
    assert_eq!(root["theme"], json!("dark"));
    assert_eq!(root["mcpServers"]["other"]["command"], json!("x"));
    assert_json_robco(&root, false);
}

#[test]
fn claude_remove_only_robco() {
    let installed =
        claude::upsert_claude(Some(r#"{"mcpServers":{"other":{"command":"x"}}}"#)).unwrap();
    let removed = claude::remove_claude(&installed).unwrap().unwrap();
    let root = json_root(&removed);

    assert!(root["mcpServers"].get("robco").is_none());
    assert_eq!(root["mcpServers"]["other"]["command"], json!("x"));
    assert!(claude::remove_claude(&removed).unwrap().is_none());
}

#[test]
fn claude_create_from_missing() {
    let created = claude::upsert_claude(None).unwrap();
    let root = json_root(&created);

    assert_json_robco(&root, false);
}

#[test]
fn claude_empty_existing_content_errors() {
    assert!(claude::upsert_claude(Some("")).is_err());
}

#[test]
fn claude_malformed_content_errors() {
    assert!(claude::upsert_claude(Some("{")).is_err());
    assert!(claude::remove_claude("{").is_err());
}

#[test]
fn claude_non_object_mcp_servers_is_overwritten() {
    let created = claude::upsert_claude(Some(r#"{"mcpServers":5}"#)).unwrap();
    let root = json_root(&created);

    assert_json_robco(&root, false);
}

#[test]
fn codex_upsert_is_idempotent_and_preserves_comments() {
    let once = codex::upsert_codex(Some("# keep\n[profile]\nname = \"main\"\n")).unwrap();
    let twice = codex::upsert_codex(Some(&once)).unwrap();
    let doc = codex_doc(&once);

    assert_eq!(once, twice);
    assert!(once.starts_with("# keep"));
    assert_eq!(doc["profile"]["name"].as_str(), Some("main"));
    assert_codex_robco(&doc);
}

#[test]
fn codex_remove_only_robco() {
    let installed = codex::upsert_codex(Some("[mcp_servers.other]\ncommand = \"x\"\n")).unwrap();
    let removed = codex::remove_codex(&installed).unwrap().unwrap();
    let doc = codex_doc(&removed);

    assert!(doc["mcp_servers"].get("robco").is_none());
    assert_eq!(doc["mcp_servers"]["other"]["command"].as_str(), Some("x"));
    assert!(codex::remove_codex(&removed).unwrap().is_none());
}

#[test]
fn codex_create_from_missing() {
    let created = codex::upsert_codex(None).unwrap();
    let doc = codex_doc(&created);

    assert_codex_robco(&doc);
}

#[test]
fn codex_empty_existing_content_creates_entry() {
    let created = codex::upsert_codex(Some("")).unwrap();
    let doc = codex_doc(&created);

    assert_codex_robco(&doc);
}

#[test]
fn codex_malformed_content_errors() {
    assert!(codex::upsert_codex(Some("[")).is_err());
    assert!(codex::remove_codex("[").is_err());
}

#[test]
fn openclaw_upsert_is_idempotent_and_preserves_unrelated_keys() {
    let once = openclaw::upsert_openclaw(Some(
        r#"{"window":true,"mcpServers":{"other":{"command":"x"}}}"#,
    ))
    .unwrap();
    let twice = openclaw::upsert_openclaw(Some(&once)).unwrap();
    let root = json_root(&once);

    assert_eq!(once, twice);
    assert_eq!(root["window"], json!(true));
    assert_eq!(root["mcpServers"]["other"]["command"], json!("x"));
    assert_json_robco(&root, true);
}

#[test]
fn openclaw_remove_only_robco() {
    let installed =
        openclaw::upsert_openclaw(Some(r#"{"mcpServers":{"other":{"command":"x"}}}"#)).unwrap();
    let removed = openclaw::remove_openclaw(&installed).unwrap().unwrap();
    let root = json_root(&removed);

    assert!(root["mcpServers"].get("robco").is_none());
    assert_eq!(root["mcpServers"]["other"]["command"], json!("x"));
    assert!(openclaw::remove_openclaw(&removed).unwrap().is_none());
}

#[test]
fn openclaw_create_from_missing() {
    let created = openclaw::upsert_openclaw(None).unwrap();
    let root = json_root(&created);

    assert_json_robco(&root, true);
}

#[test]
fn openclaw_empty_existing_content_errors() {
    assert!(openclaw::upsert_openclaw(Some("")).is_err());
}

#[test]
fn openclaw_malformed_content_errors() {
    assert!(openclaw::upsert_openclaw(Some("{")).is_err());
    assert!(openclaw::remove_openclaw("{").is_err());
}

#[test]
fn openclaw_non_object_mcp_servers_is_overwritten() {
    let created = openclaw::upsert_openclaw(Some(r#"{"mcpServers":5}"#)).unwrap();
    let root = json_root(&created);

    assert_json_robco(&root, true);
}

fn json_root(content: &str) -> Value {
    serde_json::from_str(content).unwrap()
}

fn assert_json_robco(root: &Value, has_transport: bool) {
    let robco = &root["mcpServers"]["robco"];

    assert_eq!(robco["command"], json!("robco"));
    assert_eq!(robco["args"], json!(["mcp-stdio"]));
    if has_transport {
        assert_eq!(robco["transport"], json!("stdio"));
    }
}

fn codex_doc(content: &str) -> DocumentMut {
    DocumentMut::from_str(content).unwrap()
}

fn assert_codex_robco(doc: &DocumentMut) {
    let robco = &doc["mcp_servers"]["robco"];
    let args = robco["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(robco["command"].as_str(), Some("robco"));
    assert_eq!(args, ["mcp-stdio"]);
}
