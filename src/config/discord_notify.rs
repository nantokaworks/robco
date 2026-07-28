//! Detects the seven per-event `notify_*` booleans retired alongside
//! `notify_level` becoming the sole gate for which Discord events post
//! (dropr task #338). `DiscordConfig` no longer declares these fields, so a
//! saved config still carrying one deserializes fine — serde ignores
//! unknown keys by default — but silently drops the override. This module
//! is how [`crate::config::Config::load_at`] surfaces that instead of
//! letting it vanish unremarked.

const LEGACY_KEYS: [&str; 7] = [
    "notify_escalation",
    "notify_pr_opened",
    "notify_merged",
    "notify_circuit",
    "notify_worker_blocked",
    "notify_task_started",
    "notify_task_finished",
];

/// Scans the raw config JSON for `overseer.discord` keys in [`LEGACY_KEYS`],
/// returning a one-time notice when any are present. `notify_pr_opened` gets
/// a specific hint: it gates the `all`-tier `pr_opened` event, `summary` is
/// the default level, so an operator relying on `notify_pr_opened = true` is
/// the one most likely to be surprised when it silently stops mattering.
pub(super) fn detect_legacy_keys(raw: &str) -> Option<String> {
    let root: serde_json::Value = serde_json::from_str(raw).ok()?;
    let discord = root.get("overseer")?.get("discord")?.as_object()?;
    let present: Vec<&str> = LEGACY_KEYS
        .into_iter()
        .filter(|key| discord.contains_key(*key))
        .collect();
    if present.is_empty() {
        return None;
    }
    let mut notice = format!(
        "config: overseer.discord still sets retired per-event notify override(s) ({}); \
         they no longer have any effect — notify_level alone now decides which events post.",
        present.join(", ")
    );
    if present.contains(&"notify_pr_opened") {
        notice.push_str(
            " If you relied on notify_pr_opened to control PR-opened notifications, \
             set notify_level to \"all\" instead.",
        );
    }
    Some(notice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_legacy_keys_yields_no_notice() {
        let raw = r#"{"overseer": {"discord": {"notify_level": "summary"}}}"#;
        assert_eq!(detect_legacy_keys(raw), None);
    }

    #[test]
    fn a_present_legacy_key_is_named_in_the_notice() {
        let raw = r#"{"overseer": {"discord": {"notify_circuit": true}}}"#;
        let notice = detect_legacy_keys(raw).expect("a removed key present is reported");
        assert!(notice.contains("notify_circuit"), "{notice}");
    }

    #[test]
    fn a_null_legacy_key_still_counts_as_present() {
        let raw = r#"{"overseer": {"discord": {"notify_merged": null}}}"#;
        let notice = detect_legacy_keys(raw).expect("a present-but-null key is still reported");
        assert!(notice.contains("notify_merged"), "{notice}");
    }

    #[test]
    fn notify_pr_opened_gets_the_specific_hint() {
        let raw = r#"{"overseer": {"discord": {"notify_pr_opened": true}}}"#;
        let notice = detect_legacy_keys(raw).unwrap();
        assert!(notice.contains("notify_pr_opened"), "{notice}");
        assert!(notice.contains("notify_level"), "{notice}");
    }

    #[test]
    fn a_config_with_no_discord_section_yields_no_notice() {
        assert_eq!(detect_legacy_keys("{}"), None);
    }
}
