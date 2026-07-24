use super::*;

#[test]
fn root_insets_top_and_sides_and_keeps_footer_on_last_row() {
    let layout = root(Rect::new(0, 0, 80, 24));

    assert_eq!(layout.body, Rect::new(1, 1, 78, 22));
    // Footer sits on the last terminal row, inset one column on each side.
    assert_eq!(layout.footer, Rect::new(1, 23, 78, 1));
}

#[test]
fn root_inset_is_relative_to_a_non_origin_area() {
    let layout = root(Rect::new(5, 3, 40, 10));

    assert_eq!(layout.body, Rect::new(6, 4, 38, 8));
    assert_eq!(layout.footer, Rect::new(6, 12, 38, 1));
}

#[test]
fn root_does_not_underflow_on_tiny_terminals() {
    for (width, height) in [(0, 0), (1, 1), (2, 1), (1, 2), (2, 2), (3, 3)] {
        let layout = root(Rect::new(0, 0, width, height));

        let expected_width = width.saturating_sub(2);
        assert_eq!(layout.body.width, expected_width);
        assert_eq!(layout.footer.width, expected_width);
        // The inset area never extends past the original bottom edge
        // (the y+1 shift itself is the only row on a height-0 area).
        assert!(layout.footer.bottom() <= height.max(1));
    }
}

#[test]
fn footer_caret_stays_right_of_brand_at_normal_and_narrow_widths() {
    let normal = footer(Rect::new(1, 23, 78, 1));
    let brand_width = ("ROBCO ".len() + normal.version.chars().count()) as u16;
    let last_brand_x = normal.zones.ident.x + brand_width - 1;

    assert!(normal.caret.0 > last_brand_x);
    assert!(normal.caret.0 < normal.zones.ident.right());

    // Only one blank column fits after the brand, so the preferred
    // one-column gap is clamped to the final cell in the ident zone.
    let narrow_area = Rect::new(3, 7, brand_width + 1, 1);
    let narrow = footer(narrow_area);
    let narrow_last_brand_x = narrow.zones.ident.x + brand_width - 1;

    assert!(narrow.caret.0 > narrow_last_brand_x);
    assert!(narrow.caret.0 < narrow.zones.ident.right());
    assert_eq!(narrow.caret.0, narrow.zones.ident.right() - 1);
    assert!(narrow_area.contains(narrow.caret.into()));

    let no_spare_area = Rect::new(3, 7, brand_width, 1);
    let no_spare = footer(no_spare_area);

    assert!(no_spare_area.contains(no_spare.caret.into()));
    assert!(no_spare.caret.0 >= no_spare.zones.ident.x);
    assert!(no_spare.caret.0 < no_spare.zones.ident.right());

    let shorter_area = Rect::new(3, 7, brand_width.saturating_sub(2), 1);
    let shorter = footer(shorter_area);

    assert!(shorter_area.contains(shorter.caret.into()));
}

#[test]
fn tree_stack_reserves_overseer_frame_above_projects() {
    let layout = tree_stack(Rect::new(2, 4, 32, 20), 8);

    assert_eq!(layout.overseer, Rect::new(2, 4, 32, 8));
    assert_eq!(layout.projects, Rect::new(2, 12, 32, 12));
}

#[test]
fn tree_stack_gives_projects_full_height_without_overseer() {
    let tree = Rect::new(2, 4, 32, 20);
    let layout = tree_stack(tree, 0);

    assert_eq!(layout.overseer, Rect::default());
    assert_eq!(layout.projects, tree);
}

#[test]
fn overseer_frame_height_clamps_content_plus_spacer() {
    assert_eq!(overseer_frame_height(0), OVERSEER_FRAME_MIN_HEIGHT);
    assert_eq!(overseer_frame_height(5), 6);
    assert_eq!(overseer_frame_height(usize::MAX), OVERSEER_FRAME_MAX_HEIGHT);
}
