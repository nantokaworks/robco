//! Japanese translation table, split by the UI area each entry belongs to so
//! no single file grows past the project's file-size limit.
//!
//! Every table maps a literal English source string (the exact text a call
//! site passes to `locale::t` / `locale::fmt`) to its Japanese rendering.
//! There is no requirement for coverage to be exhaustive: `locale::t` falls
//! back to the English source for anything missing here, so an omission is
//! safe, just untranslated.
//!
//! Label-vs-content rule (dropr:377): UI item labels — headers, field names,
//! and status chrome — stay English and MUST NOT get a table entry; that
//! fallback is how they render. Only content is translated: full sentences,
//! action-result messages, hints, error text, empty-state lines, relative
//! ages, and key-hint instructions.

mod actions;
mod dialog;
mod help;
mod input;
mod merge_dialog;
mod misc;
mod overseer;
mod preview;
mod summary;

pub(super) fn lookup(en: &str) -> Option<&'static str> {
    help::lookup(en)
        .or_else(|| dialog::lookup(en))
        .or_else(|| actions::lookup(en))
        .or_else(|| input::lookup(en))
        .or_else(|| summary::lookup(en))
        .or_else(|| overseer::lookup(en))
        .or_else(|| preview::lookup(en))
        .or_else(|| merge_dialog::lookup(en))
        .or_else(|| misc::lookup(en))
}
