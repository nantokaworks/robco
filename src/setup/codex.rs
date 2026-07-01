use std::{fs, path::PathBuf, str::FromStr};

use toml_edit::{Array, DocumentMut, Item, Table, Value};

use crate::Result;

use super::Action;

pub fn install() -> Result<Action> {
    let path = config_path()?;
    let before = super::read_optional_config(&path)?;
    let after = upsert_codex(before.as_deref())?;
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
    let Some(after) = remove_codex(&content)? else {
        return Ok(Action::NotPresent);
    };
    write_config(&path, &after)?;
    Ok(Action::Removed)
}

pub fn upsert_codex(content: Option<&str>) -> Result<String> {
    let mut doc = DocumentMut::from_str(content.unwrap_or(""))?;
    ensure_table(&mut doc, "mcp_servers");
    ensure_nested_table(&mut doc, "mcp_servers", "robco");

    let robco = doc["mcp_servers"]["robco"]
        .as_table_mut()
        .expect("robco table was ensured");
    robco["command"] = toml_edit::value("robco");
    robco["args"] = Item::Value(Value::Array(mcp_args()));

    Ok(doc.to_string())
}

pub fn remove_codex(content: &str) -> Result<Option<String>> {
    let mut doc = DocumentMut::from_str(content)?;
    let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_like_mut) else {
        return Ok(None);
    };
    if servers.remove("robco").is_none() {
        return Ok(None);
    }
    Ok(Some(doc.to_string()))
}

fn has_robco(content: &str) -> Result<bool> {
    let doc = DocumentMut::from_str(content)?;
    Ok(doc
        .get("mcp_servers")
        .and_then(|servers| servers.get("robco"))
        .is_some())
}

fn ensure_table(doc: &mut DocumentMut, name: &str) {
    if !doc.get(name).is_some_and(Item::is_table_like) {
        doc[name] = Item::Table(Table::new());
    }
}

fn ensure_nested_table(doc: &mut DocumentMut, parent: &str, name: &str) {
    let parent = doc[parent]
        .as_table_like_mut()
        .expect("parent table was ensured");
    if !parent.get(name).is_some_and(Item::is_table_like) {
        parent.insert(name, Item::Table(Table::new()));
    }
}

fn mcp_args() -> Array {
    let mut args = Array::new();
    args.push("mcp-stdio");
    args
}

fn config_path() -> Result<PathBuf> {
    Ok(super::home_dir()?.join(".codex").join("config.toml"))
}

fn write_config(path: &PathBuf, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}
