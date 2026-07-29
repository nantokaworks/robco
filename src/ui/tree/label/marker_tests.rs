//! `ManagementMarker` semantics: which glyph each state renders (round
//! fallback vs. the `#362` Nerdfont pictograph pair), the ownership/mode
//! pairing `of` reads, and the `unless_matching` blanking rule. Prefix
//! *layout* (connector placement, column stability) lives in
//! `prefix_tests.rs`.

use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use super::*;

const ALL: [ManagementMarker; 3] = [
    ManagementMarker::Auto,
    ManagementMarker::Manual,
    ManagementMarker::Unmanaged,
];

fn glyph_at(management: ManagementMarker, icon: ProjectIcon) -> String {
    agent_row_prefix(
        ">",
        management,
        icon,
        &[],
        true,
        TreeHandle::Leaf,
        Style::default(),
        Style::default(),
    )[1]
    .content
    .to_string()
}

/// Auto, Manual, and unmanaged are three distinguishable renderings under the
/// round fallback — the old prefix drew Manual and unmanaged identically even
/// though `g` treats them as separate steps.
#[test]
fn the_three_management_states_render_three_distinct_markers() {
    assert_eq!(glyph_at(ManagementMarker::Auto, ProjectIcon::None), "●");
    assert_eq!(glyph_at(ManagementMarker::Manual, ProjectIcon::None), "○");
    assert_eq!(
        glyph_at(ManagementMarker::Unmanaged, ProjectIcon::None),
        " "
    );
}

/// `#362`: under `project_icon = "nerdfont"` the marker switches to a
/// bolt/hand pictograph pair instead of the round fallback, and the pair
/// stays distinct from both each other and the round glyphs — an operator
/// without a patched font never sees these codepoints, since `ProjectIcon`
/// gates them the same way it gates the folder marker.
#[test]
fn nerdfont_project_icon_swaps_in_a_distinct_pictograph_pair() {
    let auto = glyph_at(ManagementMarker::Auto, ProjectIcon::Nerdfont);
    let manual = glyph_at(ManagementMarker::Manual, ProjectIcon::Nerdfont);

    assert_ne!(auto, manual);
    assert_ne!(auto, "●");
    assert_ne!(manual, "○");
}

/// `ProjectIcon::Emoji` is not a Nerdfont opt-in, so it falls back to the same
/// round glyphs as `ProjectIcon::None` rather than a third, unrelated
/// pictograph family.
#[test]
fn emoji_project_icon_keeps_the_round_fallback() {
    for management in ALL {
        assert_eq!(
            glyph_at(management, ProjectIcon::Emoji),
            glyph_at(management, ProjectIcon::None)
        );
    }
}

/// `#362`: an Auto agent under an Auto repo — the common case — used to blank
/// out via `unless_matching` and render identically to an unmanaged
/// worktree. Auto is now exempt from blanking; only a Manual agent matching a
/// Manual repo still blanks, since that case can only collide with
/// Unmanaged (Auto is always drawn) rather than reintroducing the
/// Auto/Unmanaged ambiguity this fixes.
#[test]
fn only_a_manual_agent_matching_a_manual_repo_blanks() {
    assert_eq!(
        ManagementMarker::Auto.unless_matching(ManagementMarker::Auto),
        ManagementMarker::Auto
    );
    assert_eq!(
        ManagementMarker::Manual.unless_matching(ManagementMarker::Manual),
        ManagementMarker::Unmanaged
    );
    assert_eq!(
        ManagementMarker::Manual.unless_matching(ManagementMarker::Auto),
        ManagementMarker::Manual
    );
    assert_eq!(
        ManagementMarker::Auto.unless_matching(ManagementMarker::Manual),
        ManagementMarker::Auto
    );
}

/// Ownership and mode are read together: a worktree adopted from
/// `worktree_root` carries a stale `Manual` while nobody owns it.
#[test]
fn only_an_overseer_owned_row_reads_its_management_mode() {
    let overseer = Some(crate::overseer::OVERSEER_AGENT_ID);
    let of = ManagementMarker::of;
    assert_eq!(of(overseer, ManagementMode::Auto), ManagementMarker::Auto);
    assert_eq!(
        of(overseer, ManagementMode::Manual),
        ManagementMarker::Manual
    );
    for unowned in [None, Some("some-other-agent")] {
        for mode in [ManagementMode::Auto, ManagementMode::Manual] {
            assert_eq!(of(unowned, mode), ManagementMarker::Unmanaged);
        }
    }
}

/// Every state spends exactly one column, so the cell the prefix reserves is
/// the same width whichever state lands in it — under every `ProjectIcon`
/// setting, since the Nerdfont pair (`#362`) must hold the same single-column
/// invariant as the round fallback.
#[test]
fn every_management_marker_spends_exactly_one_column() {
    for icon in [ProjectIcon::None, ProjectIcon::Nerdfont, ProjectIcon::Emoji] {
        for management in ALL {
            assert_eq!(UnicodeWidthStr::width(management.glyph(icon)), 1);
        }
    }
}
