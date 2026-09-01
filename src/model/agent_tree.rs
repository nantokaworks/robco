use super::AgentNode;

/// One agent's position in the identity tree's display order: its flat
/// index, its nesting depth, whether it is the last child among its
/// siblings, and — for each ancestor level above it, root first — whether
/// that ancestor still has a later sibling below. The tree connector needs
/// the last field to know, per row, whether to draw a continuing `│` guide
/// or leave that column blank while it descends through this row's subtree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    pub index: usize,
    pub depth: usize,
    pub is_last: bool,
    pub ancestor_continues: Vec<bool>,
}

/// Agent rows in display order, each carrying the sibling bookkeeping the
/// tree connector needs. `agent_order` is a depth-only view of the same
/// traversal for callers that don't render connectors.
pub fn agent_rows(agents: &[AgentNode]) -> Vec<AgentRow> {
    use std::collections::{HashMap, HashSet};

    let by_id: HashMap<&str, usize> = agents
        .iter()
        .enumerate()
        .map(|(index, agent)| (agent.id.as_str(), index))
        .collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); agents.len()];
    for (index, agent) in agents.iter().enumerate() {
        if let Some(parent) = agent
            .parent_agent_id
            .as_deref()
            .and_then(|id| by_id.get(id).copied())
        {
            children[parent].push(index);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn visit(
        index: usize,
        depth: usize,
        is_last: bool,
        ancestor_continues: &[bool],
        children: &[Vec<usize>],
        visited: &mut HashSet<usize>,
        ordered: &mut Vec<AgentRow>,
    ) {
        if !visited.insert(index) {
            return;
        }
        ordered.push(AgentRow {
            index,
            depth,
            is_last,
            ancestor_continues: ancestor_continues.to_vec(),
        });
        let kids = &children[index];
        let mut deeper = ancestor_continues.to_vec();
        deeper.push(!is_last);
        let last_child = kids.len().saturating_sub(1);
        for (position, &child) in kids.iter().enumerate() {
            visit(
                child,
                depth + 1,
                position == last_child,
                &deeper,
                children,
                visited,
                ordered,
            );
        }
    }

    let mut visited = HashSet::new();
    let mut ordered = Vec::with_capacity(agents.len());
    let roots: Vec<usize> = (0..agents.len())
        .filter(|&index| {
            !agents[index]
                .parent_agent_id
                .as_deref()
                .is_some_and(|id| by_id.contains_key(id))
        })
        .collect();
    let last_root = roots.len().saturating_sub(1);
    for (position, &index) in roots.iter().enumerate() {
        visit(
            index,
            0,
            position == last_root,
            &[],
            &children,
            &mut visited,
            &mut ordered,
        );
    }
    // Orphans left by a cycle where every member claims a parent: none of them
    // qualified as a root above, so nothing would visit them without this pass.
    for index in 0..agents.len() {
        if !visited.contains(&index) {
            visit(index, 0, true, &[], &children, &mut visited, &mut ordered);
        }
    }
    ordered
}

/// The single row for one agent index, for call sites that already know
/// which agent they're rendering rather than walking the full order.
pub fn agent_row(agents: &[AgentNode], index: usize) -> AgentRow {
    agent_rows(agents)
        .into_iter()
        .find(|row| row.index == index)
        .unwrap_or(AgentRow {
            index,
            depth: 0,
            is_last: true,
            ancestor_continues: Vec::new(),
        })
}

/// Agent indices and identity-tree depths in display order.
pub fn agent_order(agents: &[AgentNode]) -> Vec<(usize, usize)> {
    agent_rows(agents)
        .into_iter()
        .map(|row| (row.index, row.depth))
        .collect()
}

#[cfg(test)]
#[path = "agent_tree_tests.rs"]
mod tests;
