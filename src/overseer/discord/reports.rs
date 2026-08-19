//! Read-only status reporting shared by Discord's `!status`/`!workers`/
//! `!tasks`/`!log` and MCP's equivalent tools — split out of `actions.rs` to
//! keep it under the file-size limit, and `pub(crate)` so `crate::mcp::tools`
//! can call the same functions instead of holding its own copies.

use super::respond::{bounded_rows, code_block};
use crate::{
    overseer::{
        config::OverseerConfig,
        is_overseer_child,
        ledger::{Ledger, LedgerPhase},
        logging,
    },
    registry::Registry,
};

pub(crate) fn status() -> crate::Result<String> {
    let config = crate::config::Config::load()?;
    let ledger = Ledger::load()?;
    let active = ledger
        .entries
        .iter()
        .filter(|entry| !terminal(entry.phase))
        .count();
    Ok(status_line(&config.overseer, active))
}

/// Render the `status` reply. Kept in step with `robco overseer status`: it
/// reports only toggles the daemon honours, so the surfaces cannot disagree
/// about whether the Overseer is running.
fn status_line(config: &OverseerConfig, active: usize) -> String {
    [
        format!("**automerge** {}", on_off(config.auto_merge)),
        format!("**autonomy** {}", config.autonomy_level.label()),
        format!("**workers** {active}"),
    ]
    .join("\n")
}

pub(crate) fn workers() -> crate::Result<String> {
    let registry = Registry::load()?;
    let rows: Vec<_> = registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .filter(|agent| is_overseer_child(agent.parent_agent_id.as_deref()))
        .map(|agent| format!("`{}` {}", agent.id, agent.status.badge()))
        .collect();
    Ok(if rows.is_empty() {
        "no overseer workers".into()
    } else {
        bounded_rows(&rows)
    })
}

pub(crate) fn tasks() -> crate::Result<String> {
    let rows: Vec<_> = Ledger::load()?
        .entries
        .into_iter()
        .map(|entry| {
            format!(
                "{} {} {}",
                entry.display_id,
                entry.task_id,
                entry.phase.label()
            )
        })
        .collect();
    Ok(if rows.is_empty() {
        "no tasks".into()
    } else {
        code_block(&rows)
    })
}

pub(crate) fn format_decisions(limit: usize) -> crate::Result<String> {
    let rows: Vec<_> = logging::tail(limit)?
        .into_iter()
        .map(|entry| {
            format!(
                "{} {} {}",
                entry.at.to_rfc3339(),
                entry.kind.label(),
                entry.reason
            )
        })
        .collect();
    Ok(if rows.is_empty() {
        "no decisions".into()
    } else {
        code_block(&rows)
    })
}

fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[cfg(test)]
#[path = "reports_tests.rs"]
mod tests;
