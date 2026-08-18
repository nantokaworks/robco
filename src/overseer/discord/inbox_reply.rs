//! `!inbox`: the same rows the TUI's Inbox category shows, built through
//! `ui::inbox::current` and `overseer::remedy` — the same functions the TUI
//! itself calls — so the two surfaces cannot disagree about what a row means
//! or what to do about it. See dropr:465.

use crate::{
    config::Config,
    locale::{self, Locale},
    registry::Registry,
    ui::inbox,
};

use super::respond::code_block;

pub(super) fn inbox() -> crate::Result<String> {
    let registry = Registry::load()?;
    let aggregated = inbox::current(&registry)?;
    let locale = Locale::resolve(Config::load()?.language.as_deref());
    Ok(format_inbox(&aggregated.items, locale))
}

// `Remedy.means` / `.next` are message content, not UI chrome, so they
// follow the configured `language` the same way every other Discord reply
// does — see the repo's TUI/Discord localization policy. `Move`'s tag stays
// untranslated English, matching that same policy's row-label rule.
fn format_inbox(items: &[inbox::InboxItem], locale: Locale) -> String {
    if items.is_empty() {
        return "inbox is empty".into();
    }
    let rows: Vec<_> = items
        .iter()
        .map(|item| {
            let remedy = item.remedy();
            format!(
                "{} {} — {} {}",
                remedy.tag(),
                item.target_id,
                locale::t(locale, remedy.means),
                locale::t(locale, remedy.next),
            )
        })
        .collect();
    code_block(&rows)
}

#[cfg(test)]
#[path = "inbox_reply_tests.rs"]
mod tests;
