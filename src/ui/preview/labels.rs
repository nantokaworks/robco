use crate::{model::Selection, registry::Registry};

pub(super) fn ai_label(
    selection: Option<Selection>,
    registry: &Registry,
    default_program: &str,
) -> String {
    let raw = match selection {
        Some(Selection::Agent { repo, agent }) => {
            let agent = &registry.repos[repo].agents[agent];
            agent.profile.clone().unwrap_or_else(|| {
                agent
                    .program
                    .split_whitespace()
                    .next()
                    .unwrap_or("AI")
                    .to_string()
            })
        }
        _ => default_program.to_string(),
    };
    raw.to_uppercase()
}
