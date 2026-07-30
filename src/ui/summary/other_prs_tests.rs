use super::*;
use crate::overseer::other_prs::RepoOtherPrs;

fn pr(number: u64, state: &str) -> OtherPr {
    OtherPr {
        number,
        title: format!("bump dependency {number}"),
        author: "app/dependabot".into(),
        url: format!("https://example.test/pull/{number}"),
        head_ref_name: format!("dependabot/{number}"),
        mergeable_state: state.into(),
        closes_task: None,
    }
}

fn rendered(other_prs: &OtherPrs) -> Vec<String> {
    other_prs_section(other_prs, Path::new("/repo"), 40, Locale::En)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}

/// The acceptance criterion this guards: a repository with no third-party
/// pull requests shows no new noise, not an empty heading.
#[test]
fn a_repo_with_no_other_prs_renders_no_section_at_all() {
    assert!(rendered(&OtherPrs::default()).is_empty());
}

#[test]
fn open_pull_requests_list_number_title_author_and_state() {
    let mut other_prs = OtherPrs::default();
    other_prs.repos.insert(
        "/repo".into(),
        RepoOtherPrs {
            polled_at: chrono::Utc::now(),
            prs: vec![pr(743, "CLEAN"), pr(742, "UNSTABLE")],
        },
    );

    let lines = rendered(&other_prs);
    assert!(lines.iter().any(|line| line == "PULL REQUESTS"));
    assert!(lines.iter().any(|line| line.contains("#742")
        && line.contains("by app/dependabot")
        && line.contains("UNSTABLE")));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("#743") && line.contains("CLEAN"))
    );
}

/// #742 sorts before #743 regardless of insertion order — the operator scans
/// the list in a stable, predictable order.
#[test]
fn pull_requests_are_sorted_by_number() {
    let mut other_prs = OtherPrs::default();
    other_prs.repos.insert(
        "/repo".into(),
        RepoOtherPrs {
            polled_at: chrono::Utc::now(),
            prs: vec![pr(743, "CLEAN"), pr(742, "UNSTABLE")],
        },
    );

    let numbered: Vec<String> = rendered(&other_prs)
        .into_iter()
        .filter(|line| line.starts_with('#'))
        .collect();
    let positions: Vec<usize> = ["#742", "#743"]
        .iter()
        .map(|needle| {
            numbered
                .iter()
                .position(|line| line.starts_with(needle))
                .unwrap()
        })
        .collect();
    assert!(positions[0] < positions[1]);
}

/// A failing pull request must not read the same as a passing one.
#[test]
fn a_non_clean_state_is_visibly_distinct_from_clean() {
    let mut other_prs = OtherPrs::default();
    other_prs.repos.insert(
        "/repo".into(),
        RepoOtherPrs {
            polled_at: chrono::Utc::now(),
            prs: vec![pr(742, "UNSTABLE"), pr(743, "CLEAN")],
        },
    );

    let sections = other_prs_section(&other_prs, Path::new("/repo"), 40, Locale::En);
    let unstable_style = sections
        .iter()
        .find(|line| line.spans.iter().any(|span| span.content == "  UNSTABLE"))
        .and_then(|line| line.spans.last())
        .map(|span| span.style);
    let clean_style = sections
        .iter()
        .find(|line| line.spans.iter().any(|span| span.content == "  CLEAN"))
        .and_then(|line| line.spans.last())
        .map(|span| span.style);
    assert_ne!(unstable_style, clean_style);
}

/// A repository is looked up by its exact path, not by any repository
/// that happens to have pull requests cached.
#[test]
fn a_different_repositorys_pull_requests_do_not_leak_in() {
    let mut other_prs = OtherPrs::default();
    other_prs.repos.insert(
        "/other-repo".into(),
        RepoOtherPrs {
            polled_at: chrono::Utc::now(),
            prs: vec![pr(1, "CLEAN")],
        },
    );
    assert!(rendered(&other_prs).is_empty());
}
