/// Focus inside a repository's dropr task-list drill-down, entered from
/// `Selection::Repo` with the INFO pane showing (dropr:475). Task rows are no
/// longer members of the outer cursor list — `App::selected` stays on the
/// repository row the whole time this is `Some`, and movement keys are
/// intercepted (`ui::input::dropr_task_drill::handle_normal`) to walk this
/// instead. `task` indexes into the same
/// `ui::summary::dropr_tasks::selectable_tasks` order `ui::actions`'s
/// dropr-task modules (walking the list, opening a body, launching it) read
/// this same list.
///
/// Reading one task's full body used to be a second state here
/// (`DroprTaskFocus::Body`); it is now `Mode::TaskBody`, a dialog drawn over
/// this list instead of a state that replaces it (dropr:501) — this cursor
/// never changes while that dialog is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DroprTaskFocus {
    pub(crate) task: usize,
}
