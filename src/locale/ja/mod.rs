//! Japanese translation table, split by the UI area each entry belongs to so
//! no single file grows past the project's file-size limit.
//!
//! Every table maps a literal English source string (the exact text a call
//! site passes to `locale::t` / `locale::fmt`) to its Japanese rendering.
//! There is no requirement for coverage to be exhaustive: `locale::t` falls
//! back to the English source for anything missing here, so an omission is
//! safe, just untranslated.
//!
//! Label-vs-content rule (dropr:377, tightened by dropr:388): UI item labels
//! — headers, field names, and status chrome — stay English and MUST NOT get
//! a table entry; that fallback is how they render. The same holds for any
//! string drawn inside a tree/sidebar row: row placeholders, short status
//! words, and category summary values are chrome, not content. Only
//! multi-word prose is translated: full sentences, action-result messages,
//! hints, error text, dialog messages, and key-hint instructions. Relative
//! ages ("{}m ago", …) are values, not chrome, and may stay translated.

mod actions;
mod dialog;
mod help;
mod humanize;
mod input;
mod merge_dialog;
mod misc;
mod overseer;
mod preview;
mod remedy;
mod remedy_merge;
mod remedy_runtime;
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
        .or_else(|| remedy::lookup(en))
        .or_else(|| remedy_merge::lookup(en))
        .or_else(|| remedy_runtime::lookup(en))
        .or_else(|| humanize::lookup(en))
}
