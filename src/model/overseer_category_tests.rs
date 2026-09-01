use super::*;

#[test]
fn category_labels_stay_english_in_every_locale() {
    use crate::locale::{Locale, t};
    for category in OverseerCategory::ALL {
        assert_eq!(t(Locale::En, category.label()), category.label());
        assert_eq!(t(Locale::Ja, category.label()), category.label());
    }
}

#[test]
fn every_category_has_a_distinct_index_in_bounds() {
    let mut seen = std::collections::HashSet::new();
    for category in OverseerCategory::ALL {
        assert!(category.index() < OverseerCategory::COUNT);
        assert!(seen.insert(category.index()), "{}", category.label());
    }
}

#[test]
fn label_never_changes_with_locale_since_it_is_a_persisted_key() {
    for category in OverseerCategory::ALL {
        assert_eq!(category.label(), category.label());
    }
    // Regression guard for the persisted `ui_state.json` key and the
    // `item_key` preview-tab memory: `label()` takes no locale parameter
    // at all, so it cannot vary by config — this test exists to fail loudly
    // if a future edit adds one.
    let _: fn(OverseerCategory) -> &'static str = OverseerCategory::label;
}
