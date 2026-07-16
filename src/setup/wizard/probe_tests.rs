use super::probe::{self, ProbeResult};

#[test]
fn injected_probe_results_render_status_and_warnings() {
    let results = probe::run_with(|name| name == "git");
    assert!(results[0].ok);
    assert!(probe::missing_required(&results));
    let mut output = Vec::new();
    probe::render(&mut output, &results).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("git ··············· OK"));
    assert!(output.contains("gh ··············· NG (warning)"));
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
