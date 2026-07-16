use super::probe::{self, ProbeResult};

#[test]
fn injected_probe_results_render_status_and_warnings() {
    let results = probe::run_with(|name, _version_flag| name == "git");
    assert!(results[0].ok);
    assert!(probe::missing_required(&results));
    let mut output = Vec::new();
    probe::render(&mut output, &results).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("git ··············· OK"));
    assert!(output.contains("gh ··············· NG (warning)"));
}

#[test]
fn tmux_probe_uses_short_version_flag() {
    let mut probed = Vec::new();
    probe::run_with(|name, version_flag| {
        probed.push((name.to_string(), version_flag.to_string()));
        true
    });
    assert_eq!(
        probed,
        [
            ("git".to_string(), "--version".to_string()),
            ("tmux".to_string(), "-V".to_string()),
            ("gh".to_string(), "--version".to_string()),
            ("dropr".to_string(), "--version".to_string()),
        ]
    );
}

#[test]
fn only_required_failures_gate_setup() {
    let warnings = [ProbeResult {
        name: "gh",
        required: false,
        ok: false,
    }];
    assert!(!probe::missing_required(&warnings));
}
