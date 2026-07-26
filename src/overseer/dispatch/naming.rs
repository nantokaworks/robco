//! Naming for the worktree, branch, and tmux session Overseer creates for a
//! worker. Every path that spawns a worker builds its slug here, so the shape
//! stays one scheme rather than one per spawn site.

/// The numbering space a task's display id belongs to. Overseer reads dropr
/// only, so `Dropr` is the sole value today; a worker's names lead with it so
/// the origin of the number is readable, and so a second source — GitHub
/// issues, say — becomes another variant here rather than a second naming
/// scheme numbering into the same space.
#[derive(Clone, Copy)]
pub(crate) enum TaskSource {
    Dropr,
}

impl TaskSource {
    /// Every known source. A display id already carrying one of these prefixes
    /// is reduced to its bare number rather than prefixed a second time.
    const ALL: &'static [Self] = &[Self::Dropr];

    /// What the worker's branch, worktree directory, and tmux session lead with.
    fn slug_prefix(self) -> &'static str {
        match self {
            Self::Dropr => "dropr",
        }
    }
}

/// tmux-safe name for a worker's worktree, branch, and session, or `None` when
/// the display id is empty and there is nothing to number the name with — the
/// caller then names the worker from its title alone.
///
/// The name leads with the task's source, so `#295` in dropr becomes
/// `dropr-295-<title>`. The caller downstream caps the result (see
/// `crate::agent`); capping falls back to the hyphen after the number, so the
/// source and number survive however long the title is.
pub(crate) fn name_slug(source: TaskSource, display_id: &str, title: &str) -> Option<String> {
    let display_id = bare_display_id(display_id);
    (!display_id.is_empty()).then(|| {
        format!(
            "{}-{display_id}-{}",
            source.slug_prefix(),
            crate::tmux::sanitize_target_part(title)
        )
    })
}

/// Reduce a display id to the bare number a name is built from. dropr renders
/// it `#295`, the dispatch ledger carries `task-295`, and a name from an earlier
/// run carries `dropr-295`; all three identify the same task, and none of them
/// may stack a second prefix onto the name.
fn bare_display_id(display_id: &str) -> &str {
    let trimmed = display_id.trim().trim_start_matches('#');
    // `task` is the shape these names carried before they led with the source.
    let carried = TaskSource::ALL
        .iter()
        .map(|source| source.slug_prefix())
        .chain(["task"]);
    for prefix in carried {
        if let Some(bare) = trimmed
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('-'))
        {
            return bare;
        }
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_leads_with_the_task_source_and_number() {
        assert_eq!(
            name_slug(TaskSource::Dropr, "#295", "Add a top-level language config").as_deref(),
            Some("dropr-295-Add-a-top-level-language-config")
        );
    }

    #[test]
    fn every_display_id_shape_reaches_the_same_name() {
        // The bare number, dropr's own rendering, the ledger's task id shape, and
        // a name carried over from an earlier run all identify task 295, and none
        // of them may stack a second prefix.
        for display_id in ["295", "#295", "  #295  ", "task-295", "dropr-295"] {
            assert_eq!(
                name_slug(TaskSource::Dropr, display_id, "Fix").as_deref(),
                Some("dropr-295-Fix"),
                "display id {display_id:?}"
            );
        }
    }

    #[test]
    fn a_task_without_a_number_is_left_to_be_named_from_its_title() {
        for display_id in ["", "   ", "#"] {
            assert_eq!(
                name_slug(TaskSource::Dropr, display_id, "Fix"),
                None,
                "display id {display_id:?}"
            );
        }
    }

    #[test]
    fn a_number_carrying_only_a_prefix_is_not_mistaken_for_a_number() {
        // `task-` alone leaves nothing to number the name with; the trailing
        // hyphen must not survive as a name of its own.
        assert_eq!(name_slug(TaskSource::Dropr, "task-", "Fix"), None);
    }
}
