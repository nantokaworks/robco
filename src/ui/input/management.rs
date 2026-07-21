use crate::{Result, model::Selection, overseer::is_overseer_child, registry::Registry};

use super::App;

pub(super) fn toggle_selected(app: &mut App) -> Result<()> {
    let Some(Selection::Agent { repo, agent }) = app.selected_item() else {
        app.show_message("g: select an overseer worker to toggle auto/manual");
        return Ok(());
    };
    let repo_path = app.registry.repos[repo].path.clone();
    let agent_id = app.registry.repos[repo].agents[agent].id.clone();
    let mut toggled = None;
    app.registry = Registry::locked_update(|registry| {
        let Some(worker) = registry
            .repos
            .iter_mut()
            .find(|repo| repo.path == repo_path)
            .and_then(|repo| repo.agents.iter_mut().find(|agent| agent.id == agent_id))
        else {
            return;
        };
        if toggle_mode(worker.parent_agent_id.as_deref(), &mut worker.management) {
            toggled = Some(worker.management);
        }
    })?;
    if let Some(mode) = toggled {
        app.show_message(format!(
            "overseer management: {}",
            format!("{mode:?}").to_ascii_lowercase()
        ));
    } else {
        app.show_message("g: only overseer-managed workers toggle auto/manual");
    }
    Ok(())
}

fn toggle_mode(parent: Option<&str>, mode: &mut crate::model::ManagementMode) -> bool {
    if !is_overseer_child(parent) {
        return false;
    }
    *mode = mode.toggled();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, model::ManagementMode, registry::Registry};

    #[test]
    fn toggle_on_non_worker_selection_explains_scope() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.overseer_visible = true;
        app.selected = 0; // OVERSEER header row — not a worker (Selection::Agent).

        toggle_selected(&mut app).unwrap();

        assert!(
            app.message
                .as_ref()
                .is_some_and(|(text, _)| text.contains("overseer worker")),
            "expected a scope hint message, got {:?}",
            app.message
        );
    }

    #[test]
    fn only_overseer_workers_toggle() {
        let mut mode = ManagementMode::Auto;
        assert!(toggle_mode(Some("overseer"), &mut mode));
        assert_eq!(mode, ManagementMode::Manual);
        assert!(toggle_mode(Some("chief"), &mut mode));
        assert_eq!(mode, ManagementMode::Auto);
        assert!(!toggle_mode(None, &mut mode));
        assert_eq!(mode, ManagementMode::Auto);
    }
}
