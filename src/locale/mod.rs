//! Resolves the free-form `config.language` value into a closed set of TUI
//! locales, and gives every render call site one lookup point to translate
//! through.
//!
//! `config.language` (`src/config/language.rs`) is deliberately free-form: it
//! is interpolated into LLM prompts as natural language, and nothing branches
//! on it there. The TUI's own chrome cannot render that way — every string is
//! either English or one specific translation — so [`Locale::resolve`] maps
//! the free-form value onto a known locale, falling back to English for
//! anything absent, blank, or unrecognized. That fallback is the same
//! guarantee `language_directive` already makes: a config it cannot place
//! changes nothing.

mod ja;

/// The set of locales the TUI ships translations for. Extending this means
/// adding a variant here, a branch in [`Locale::resolve`], and translation
/// table entries under `ja/` (or a sibling module) — no render call site
/// changes, since they all go through [`t`] / [`fmt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Locale {
    En,
    Ja,
}

impl Locale {
    /// Maps `config.language` onto a known locale. Absent, blank, and
    /// unrecognized values all resolve to [`Locale::En`] — byte-identical to
    /// today's output, matching the guarantee `language_directive` makes for
    /// the same input.
    pub(crate) fn resolve(language: Option<&str>) -> Self {
        let Some(raw) = language else {
            return Self::En;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Self::En;
        }
        let lower = trimmed.to_lowercase();
        let is_japanese = lower == "ja"
            || lower == "jp"
            || lower == "jpn"
            || lower.starts_with("ja-")
            || lower.starts_with("ja_")
            || lower.contains("japan")
            || trimmed.contains("日本語")
            || trimmed.contains("にほんご");
        if is_japanese { Self::Ja } else { Self::En }
    }
}

/// The single lookup point every localized render call site goes through.
///
/// `en` is both the key and the fallback: [`Locale::En`], and any
/// [`Locale::Ja`] string with no table entry, render `en` unchanged. That
/// makes adding a translation a matter of filling in a table entry, never a
/// matter of touching the call site, and guarantees an untranslated string is
/// never worse than the English original.
pub(crate) fn t(locale: Locale, en: &'static str) -> &'static str {
    match locale {
        Locale::En => en,
        Locale::Ja => ja::lookup(en).unwrap_or(en),
    }
}

/// [`t`] for a template carrying positional `{}` placeholders, filled in the
/// order given. Plain `format!` cannot take a runtime string as its format
/// spec, so a translated template is threaded through this instead —
/// translations are free to reorder surrounding words, just not the
/// placeholders themselves.
pub(crate) fn fmt(locale: Locale, template: &'static str, args: &[&str]) -> String {
    let mut rendered = t(locale, template).to_string();
    for arg in args {
        if let Some(pos) = rendered.find("{}") {
            rendered.replace_range(pos..pos + 2, arg);
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_language_resolves_to_english() {
        assert_eq!(Locale::resolve(None), Locale::En);
    }

    #[test]
    fn blank_language_resolves_to_english() {
        assert_eq!(Locale::resolve(Some("")), Locale::En);
        assert_eq!(Locale::resolve(Some("   \t ")), Locale::En);
    }

    #[test]
    fn unrecognized_language_resolves_to_english() {
        assert_eq!(Locale::resolve(Some("Brazilian Portuguese")), Locale::En);
        assert_eq!(Locale::resolve(Some("French")), Locale::En);
    }

    #[test]
    fn japanese_aliases_resolve_to_japanese() {
        for alias in ["Japanese", "japanese", "日本語", "ja", "JP", "ja-JP"] {
            assert_eq!(Locale::resolve(Some(alias)), Locale::Ja, "{alias}");
        }
    }

    #[test]
    fn an_untranslated_string_falls_back_to_english_in_every_locale() {
        let untranslated = "this string has no table entry anywhere";
        assert_eq!(t(Locale::En, untranslated), untranslated);
        assert_eq!(t(Locale::Ja, untranslated), untranslated);
    }

    #[test]
    fn a_translated_string_renders_in_japanese_and_english_stays_untouched() {
        let english = "no live session in this child worktree";
        assert_eq!(t(Locale::En, english), english);
        assert_ne!(t(Locale::Ja, english), english);
    }

    #[test]
    fn row_level_chrome_renders_english_even_in_japanese() {
        // dropr:388 — strings drawn inside tree/sidebar rows (placeholders,
        // short status words, category summary values) are chrome, not
        // content, and must not gain a `ja` table entry. See `ja/mod.rs`.
        for chrome in [
            "(no agents)",
            "(none active or recent)",
            "tasks unavailable",
            "none",
            "worker blocked",
            "waiting on the merge gate",
            "no retained channel agents yet",
            "no retained channels",
            "{}/{} actionable",
            "{} recent",
            "{} retained",
            "{} retained, {} failed",
        ] {
            assert_eq!(t(Locale::Ja, chrome), chrome, "{chrome}");
        }
    }

    #[test]
    fn fmt_fills_placeholders_in_order_after_translating_the_template() {
        assert_eq!(
            fmt(Locale::En, "branch remains: {}", &["main"]),
            "branch remains: main"
        );
        let ja_rendered = fmt(Locale::Ja, "branch remains: {}", &["main"]);
        assert!(ja_rendered.contains("main"), "{ja_rendered}");
    }
}
