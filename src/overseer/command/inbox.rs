use crate::{Result, registry::Registry};

/// Clear the Overseer Inbox from the CLI, so the operation is scriptable and
/// reachable with the TUI closed.
///
/// Aggregates the same three sources the TUI does and suppresses whatever they
/// currently produce. Suppression only — see [`crate::overseer::dismissals`] for
/// why the decision log and the ledger are never touched, and why a newer
/// escalation for a cleared target is listed again.
pub(super) fn clear_inbox() -> Result<()> {
    let inbox = crate::ui::inbox::current(&Registry::load()?)?;
    if inbox.items.is_empty() {
        println!("inbox is already empty");
        return Ok(());
    }
    let dismissed = inbox
        .items
        .iter()
        .map(|item| (item.kind.code(), item.target_id.as_str(), item.at))
        .collect::<Vec<_>>();
    crate::overseer::dismissals::dismiss(&dismissed, &inbox.targets)?;
    for (kind, target_id, _) in &dismissed {
        println!("dismissed [{kind}] {target_id}");
    }
    println!(
        "cleared {} inbox item(s); decisions.jsonl and ledger.json are unchanged",
        dismissed.len()
    );
    Ok(())
}
