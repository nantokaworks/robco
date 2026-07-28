//! Naming for the worktree, branch, and tmux session Overseer creates for a
//! worker. Every path that spawns a worker builds its slug here, so the shape
//! stays one scheme rather than one per spawn site.

/// The numbering space a task's display id belongs to. Overseer reads dropr
/// only, so `Dropr` is the sole value today; a worker's names carry it right
/// after the number so the origin of the number is readable, and so a second
/// source — GitHub issues, say — becomes another variant here rather than a
/// second naming scheme numbering into the same space.
#[derive(Clone, Copy)]
pub(crate) enum TaskSource {
    Dropr,
}

impl TaskSource {
    /// Every known source. A display id already carrying one of these prefixes
    /// is reduced to its bare number rather than prefixed a second time.
    const ALL: &'static [Self] = &[Self::Dropr];

    /// The segment right after the number in the worker's branch, worktree
    /// directory, and tmux session name.
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
/// The name leads with the task's number, so `#295` in dropr becomes
/// `295-dropr-<title>`: the number is what the operator actually scans for
/// across a column of names sharing the same repo and source, so it comes
/// first; the source segment right after it keeps the origin of the number
/// readable and leaves the numbering space open for a second task source
/// later. The caller downstream caps the result (see `crate::agent`); capping
/// falls back to the hyphen after the source, so the number and source
/// survive however long the title is.
pub(crate) fn name_slug(source: TaskSource, display_id: &str, title: &str) -> Option<String> {
    let display_id = bare_display_id(display_id);
    (!display_id.is_empty()).then(|| {
        format!(
            "{display_id}-{}-{}",
            source.slug_prefix(),
            crate::tmux::sanitize_target_part(title)
        )
    })
}

/// Reduce a display id to the bare number a name is built from. dropr renders
/// it `#295`, the dispatch ledger carries `task-295`, a name from before names
/// led with the number carries `dropr-295`, and a name from a later run carries
/// `295-dropr`; all four identify the same task, and none of them may stack a
/// second prefix onto the name.
fn bare_display_id(display_id: &str) -> &str {
    let trimmed = display_id.trim().trim_start_matches('#');
    // Leading-source shapes: `dropr-295` from before names led with the
    // number, and `task-295` from even before that.
    let leading = TaskSource::ALL
        .iter()
        .map(|source| source.slug_prefix())
        .chain(["task"]);
    for prefix in leading {
        if let Some(bare) = trimmed
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('-'))
        {
            return bare;
        }
    }
    // Trailing-source shape: `295-dropr`, what these names lead with today.
    for source in TaskSource::ALL {
        if let Some(bare) = trimmed
            .strip_suffix(source.slug_prefix())
            .and_then(|rest| rest.strip_suffix('-'))
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
    fn the_name_leads_with_the_task_number_and_source() {
        assert_eq!(
            name_slug(TaskSource::Dropr, "#295", "Add a top-level language config").as_deref(),
            Some("295-dropr-Add-a-top-level-language-config")
        );
    }

    #[test]
    fn every_display_id_shape_reaches_the_same_name() {
        // The bare number, dropr's own rendering, the ledger's task id shape, a
        // name carried over from before names led with the number, and a name
        // carried over from a later run all identify task 295, and none of them
        // may stack a second prefix.
        for display_id in [
            "295",
            "#295",
            "  #295  ",
            "task-295",
            "dropr-295",
            "295-dropr",
        ] {
            assert_eq!(
                name_slug(TaskSource::Dropr, display_id, "Fix").as_deref(),
                Some("295-dropr-Fix"),
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
    fn a_number_carrying_only_a_leading_prefix_is_not_mistaken_for_a_number() {
        // `task-` alone leaves nothing to number the name with; the trailing
        // hyphen must not survive as a name of its own.
        assert_eq!(name_slug(TaskSource::Dropr, "task-", "Fix"), None);
    }

    #[test]
    fn a_number_carrying_only_a_trailing_suffix_is_not_mistaken_for_a_number() {
        // `-dropr` alone leaves nothing to number the name with either, in the
        // shape these names lead with today.
        assert_eq!(name_slug(TaskSource::Dropr, "-dropr", "Fix"), None);
    }

    #[test]
    fn the_name_never_degrades_to_a_bare_number() {
        // tmux resolves a purely numeric `-t` target as a session id rather than
        // a name, so whatever the title contributes, the source segment must
        // always survive into the slug tmux::sanitize_target_part is fed.
        for title in ["", "   ", "295"] {
            let slug = name_slug(TaskSource::Dropr, "#295", title).unwrap();
            assert!(
                slug.parse::<u64>().is_err(),
                "slug {slug:?} degraded to a bare number for title {title:?}"
            );
        }
    }
}
