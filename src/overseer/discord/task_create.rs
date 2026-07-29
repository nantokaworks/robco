use crate::{dropr::DroprOverlay, overseer::exec::COMMAND_TIMEOUT, registry::Registry};

/// Resolves `repo` against the robco registry the same way `robco spawn
/// --repo` does, maps it onto its dropr workspace, and files the task there.
pub(super) fn create_task(
    repo: &str,
    title: &str,
    description: Option<&str>,
) -> crate::Result<String> {
    let remote = Registry::load()?
        .repos
        .into_iter()
        .find(|entry| entry.name == repo)
        .and_then(|entry| entry.remote_url)
        .ok_or_else(|| crate::Error::RepoSelectorNotFound(repo.to_string()))?;
    let (overlay, _) = DroprOverlay::load_with_status_timeout(COMMAND_TIMEOUT);
    let workspace = overlay.find_by_repo_url(&remote).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no dropr workspace found for repo: {repo}"),
        )
    })?;
    crate::dropr::task_create_timeout(&workspace.id, title, description, COMMAND_TIMEOUT)
        .map(|()| format!("created task \"{title}\" in {repo}"))
        .map_err(|error| std::io::Error::other(format!("dropr task_create failed: {error}")).into())
}
