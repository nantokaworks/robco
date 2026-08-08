use crate::{config::Config, tmux};

/// The bare dropr task number a dispatch-provided `name_slug` leads with (see
/// `crate::overseer::dispatch::naming::name_slug`), captured once here rather
/// than re-derived from `branch` or `title` at tree-render time. `None` for a
/// manual spawn, which passes no `name_slug` and so gets no number.
pub(super) fn leading_task_number(name_slug: Option<&str>) -> Option<String> {
    let digits: String = name_slug?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then_some(digits)
}

pub(super) fn naming_slug(title: &str, explicit: Option<&str>) -> String {
    let sanitized = tmux::sanitize_target_part(explicit.unwrap_or(title));
    let capped = cap_name_slug(&sanitized);
    if capped.is_empty() {
        "agent".to_string()
    } else {
        capped
    }
}

fn cap_name_slug(raw: &str) -> String {
    const MAX_CHARS: usize = 32;

    let hard_cap: String = raw.chars().take(MAX_CHARS).collect();
    let mut capped = if raw.chars().count() > MAX_CHARS {
        hard_cap.rfind('-').map_or_else(
            || hard_cap.clone(),
            |boundary| hard_cap[..boundary].to_string(),
        )
    } else {
        hard_cap.clone()
    };
    capped.truncate(capped.trim_end_matches('-').len());
    if capped.is_empty() && !raw.is_empty() {
        hard_cap
    } else {
        capped
    }
}

/// The branch name a worker spawned for `title` (with dispatch's optional
/// `name_slug`) would get in `repo_name`. Exposed so a caller can check whether
/// that branch already exists — e.g. a previous run's leftovers — *before*
/// committing to a `git worktree add` that is certain to fail on it (see
/// `crate::spawn::branch_conflict`, dropr:_ord_VtFSIiLgWpgmDAGm). Kept as the
/// single formula `create_agent_with_launch` itself calls, so the two never
/// drift apart.
pub(crate) fn worker_branch_name(
    config: &Config,
    repo_name: &str,
    title: &str,
    name_slug: Option<&str>,
) -> String {
    format!(
        "{}{}",
        resolve_branch_prefix(config, repo_name),
        naming_slug(title, name_slug)
    )
}

pub(super) fn resolve_branch_prefix(config: &Config, repo_name: &str) -> String {
    if let Some(prefix) = &config.branch_prefix {
        return prefix.clone();
    }
    // Derive `<repo>/` from the project name, sanitized so it is a valid git
    // ref. A pathological repo name (e.g. `...`) sanitizes to an empty string,
    // which would yield a `/`-prefixed (invalid) branch — fall back to the
    // historical default in that case.
    let sanitized = tmux::sanitize_target_part(repo_name);
    if sanitized.is_empty() {
        "robco/".to_string()
    } else {
        format!("{}/", sanitized)
    }
}

pub(super) fn profile_name(config: &Config) -> Option<String> {
    config
        .profiles
        .iter()
        .find(|profile| profile.name == config.default_program)
        .map(|profile| profile.name.clone())
}

#[cfg(test)]
mod tests;
