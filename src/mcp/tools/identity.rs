use serde_json::{Value, json};

use crate::{
    config::{ENV_AGENT_ID, ENV_PARENT_AGENT_ID},
    registry::Registry,
};

use super::{ToolResult, exec_err};

pub(super) fn whoami() -> ToolResult<Value> {
    whoami_with_lookup(|key| std::env::var(key).ok(), Registry::load)
}

pub(super) fn whoami_with_lookup(
    lookup: impl Fn(&str) -> Option<String>,
    load_registry: impl FnOnce() -> crate::Result<Registry>,
) -> ToolResult<Value> {
    let identity = Identity::from_lookup(lookup);
    if identity.agent_id.is_none() {
        return Ok(identity.resolve(None));
    }

    let registry = load_registry().map_err(exec_err)?;
    Ok(identity.resolve(Some(&registry)))
}

struct Identity {
    agent_id: Option<String>,
    parent_agent_id: Option<String>,
}

impl Identity {
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            agent_id: non_empty(lookup(ENV_AGENT_ID)),
            parent_agent_id: non_empty(lookup(ENV_PARENT_AGENT_ID)),
        }
    }

    fn resolve(&self, registry: Option<&Registry>) -> Value {
        let resolved = self.agent_id.as_ref().and_then(|agent_id| {
            registry?.repos.iter().find_map(|repo| {
                repo.agents
                    .iter()
                    .find(|agent| agent.id == *agent_id)
                    .map(|agent| (repo, agent))
            })
        });

        json!({
            "agent_id": self.agent_id,
            "title": resolved.map(|(_, agent)| &agent.title),
            "repo": resolved.map(|(repo, _)| &repo.name),
            "parent_agent_id": self.parent_agent_id
        })
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}
