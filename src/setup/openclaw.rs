use std::{fs, path::PathBuf};

use serde_json::{Value, json};

use crate::Result;

use super::Action;

pub fn install() -> Result<Action> {
    let path = config_path()?;
    let before = super::read_optional_config(&path)?;
    let after = upsert_openclaw(before.as_deref())?;
    let action = match before.as_deref() {
        None => Action::FileCreated,
        Some(content) if content == after => Action::AlreadyPresent,
        Some(content) if has_robco(content)? => Action::Updated,
        Some(_) => Action::Installed,
    };
    write_config(&path, &after)?;
    Ok(action)
}

pub fn uninstall() -> Result<Action> {
    let path = config_path()?;
    let Some(content) = super::read_optional_config(&path)? else {
        return Ok(Action::NotPresent);
    };
    let Some(after) = remove_openclaw(&content)? else {
        return Ok(Action::NotPresent);
    };
    write_config(&path, &after)?;
    Ok(Action::Removed)
}

pub fn upsert_openclaw(content: Option<&str>) -> Result<String> {
    let mut root = match content {
        Some(content) => serde_json::from_str(content)?,
        None => json!({}),
    };
    ensure_object(&mut root);
    let mcp_servers = root
        .as_object_mut()
        .expect("root object was ensured")
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    ensure_object(mcp_servers);
    mcp_servers
        .as_object_mut()
        .expect("mcpServers object")
        .insert(
            "robco".to_string(),
            json!({ "command": "robco", "args": ["mcp-stdio"], "transport": "stdio" }),
        );
    Ok(serde_json::to_string_pretty(&root)? + "\n")
}

pub fn remove_openclaw(content: &str) -> Result<Option<String>> {
    let mut root: Value = serde_json::from_str(content)?;
    let Some(mcp_servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    if mcp_servers.remove("robco").is_none() {
        return Ok(None);
    }
    Ok(Some(serde_json::to_string_pretty(&root)? + "\n"))
}

fn has_robco(content: &str) -> Result<bool> {
    let root: Value = serde_json::from_str(content)?;
    Ok(root
        .get("mcpServers")
        .and_then(|servers| servers.get("robco"))
        .is_some())
}

fn ensure_object(value: &mut Value) {
    if !value.is_object() {
        *value = json!({});
    }
}

fn config_path() -> Result<PathBuf> {
    Ok(super::home_dir()?.join(".openclaw").join("openclaw.json"))
}

fn write_config(path: &PathBuf, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}
