//! The primary-checkout state warning block of a repository summary.
//!
//! A primary checkout an operator or another tool left detached, or on some
//! branch other than the repository's own default branch, breaks in ways
//! that stay invisible until something else does: `ready`
//! (`overseer::release_pipeline`) skips every release with an opaque
//! reason, and a plain `git pull` errors out. This is where that state
//! becomes visible, naming both the state and the fix.

use ratatui::text::{Line, Span};

use crate::{
    locale::{Locale, fmt, t},
    model::{CheckoutState, RepoNode},
    ui::theme::DEFAULT as THEME,
};

/// Empty once the checkout is back on the repository's own default branch.
pub(super) fn checkout_branch_warning(repo: &RepoNode, locale: Locale) -> Vec<Line<'static>> {
    let Some(state) = &repo.checkout_state else {
        return Vec::new();
    };
    let message = match state {
        CheckoutState::Detached { default_branch } => fmt(
            locale,
            "HEAD is detached — press c to check out {} (clean tree only)",
            &[default_branch],
        ),
        CheckoutState::OtherBranch {
            current,
            default_branch,
        } => fmt(
            locale,
            "on branch {}, not {} — press c to check out {} (clean tree only)",
            &[current, default_branch, default_branch],
        ),
        CheckoutState::DefaultBranchUnknown => t(
            locale,
            "default branch could not be resolved — run git remote set-head origin -a",
        )
        .to_string(),
    };
    vec![Line::from(Span::styled(message, THEME.failure_style()))]
}

#[cfg(test)]
#[path = "checkout_state_tests.rs"]
mod tests;
