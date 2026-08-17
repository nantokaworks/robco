use super::*;

#[test]
fn every_command_variant_has_a_help_entry() {
    let rendered = help_message();
    assert_eq!(rendered.lines().count(), samples().len());
    for sample in samples() {
        let entry = describe(&sample);
        assert!(
            rendered.contains(entry.usage),
            "missing usage line: {}",
            entry.usage
        );
    }
}

#[test]
fn help_lists_merge_and_diff() {
    let rendered = help_message();
    assert!(rendered.contains("!merge <task>"));
    assert!(rendered.contains("!diff <task>"));
}

#[test]
fn help_message_fits_a_single_discord_message() {
    assert!(help_message().len() < 2000, "{}", help_message().len());
}
