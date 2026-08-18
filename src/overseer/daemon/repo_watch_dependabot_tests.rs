use super::*;

fn now() -> DateTime<Utc> {
    "2026-08-17T00:00:00Z".parse().unwrap()
}

fn pr(number: u64, merge_state_status: &str, age_days: i64) -> RawPr {
    RawPr {
        number,
        title: format!("Bump dep #{number}"),
        url: format!("https://github.com/nantokaworks/nex/pull/{number}"),
        author: RawAuthor {
            login: DEPENDABOT_LOGINS[0].into(),
        },
        merge_state_status: merge_state_status.into(),
        created_at: now() - chrono::Duration::days(age_days),
    }
}

#[test]
fn a_conflicted_pr_is_stale_regardless_of_age() {
    let prs = vec![pr(1, "DIRTY", 0)];
    assert_eq!(stale_prs(&prs, now(), 3).len(), 1);
}

#[test]
fn a_clean_recent_pr_is_not_stale() {
    let prs = vec![pr(1, "CLEAN", 1)];
    assert!(stale_prs(&prs, now(), 3).is_empty());
}

#[test]
fn a_clean_pr_older_than_the_threshold_is_stale() {
    let prs = vec![pr(1, "CLEAN", 3)];
    assert_eq!(stale_prs(&prs, now(), 3).len(), 1);
}

#[test]
fn a_clean_pr_exactly_at_the_threshold_boundary_is_stale() {
    let prs = vec![pr(1, "UNSTABLE", 3)];
    assert_eq!(stale_prs(&prs, now(), 3).len(), 1);
}

#[test]
fn the_marker_sorts_pr_numbers_for_stable_matching() {
    assert_eq!(marker("nex", &[1, 2, 3]), marker("nex", &[1, 2, 3]));
}

#[test]
fn the_title_carries_the_marker_verbatim_for_dedup_matching() {
    let marker = marker("nex", &[561, 562]);
    let title = title("nex", 2, &marker);
    assert!(title.contains(&marker));
}

#[test]
fn both_known_dependabot_login_forms_match() {
    assert!(is_dependabot("dependabot[bot]"));
    assert!(is_dependabot("app/dependabot"));
}

#[test]
fn an_unrelated_login_does_not_match() {
    assert!(!is_dependabot("ichi0g0y"));
    assert!(!is_dependabot("dependabot-preview[bot]"));
}
