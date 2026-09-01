use super::*;

#[test]
fn status_badges_and_glyphs_are_stable() {
    assert_eq!(Status::Running.badge(), "run");
    assert_eq!(Status::Running.glyph(), "⠿");
    assert_eq!(Status::Waiting.glyph(), "?");
    assert_eq!(Status::Done.glyph(), "✓");
    assert_eq!(Status::Idle.glyph(), "·");
    assert_eq!(Status::Dead.glyph(), "✗");
    assert_eq!(Status::BranchOnly.glyph(), "⎇");
}

#[test]
fn merge_lifecycle_glyphs_are_stable_and_distinct_from_status_glyphs() {
    let status_glyphs = [
        Status::Idle.glyph(),
        Status::Running.glyph(),
        Status::Waiting.glyph(),
        Status::Done.glyph(),
        Status::Dead.glyph(),
        Status::BranchOnly.glyph(),
    ];
    let lifecycle_glyphs = [
        MergeLifecycle::ApprovedWaiting.glyph(),
        MergeLifecycle::ChecksRunning.glyph(),
        MergeLifecycle::ChecksFailing.glyph(),
        MergeLifecycle::OnHold.glyph(),
    ];
    for glyph in lifecycle_glyphs {
        assert!(
            !status_glyphs.contains(&glyph),
            "{glyph} collides with a Status glyph"
        );
    }
    let mut seen = std::collections::HashSet::new();
    for glyph in lifecycle_glyphs {
        assert!(
            seen.insert(glyph),
            "{glyph} is not unique among MergeLifecycle glyphs"
        );
    }
}
