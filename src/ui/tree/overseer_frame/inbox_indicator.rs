use ratatui::text::Span;

use crate::{
    model::{OverseerCategory, Status},
    ui::App,
};

use super::{IndicatorState, indicator, select};

/// The Inbox row's attention signal: the same `?` waiting glyph an agent row
/// shows, lit when at least one item is actionable and absent at zero — the
/// row answers the same question an agent's `waiting` glyph does, "is
/// something here waiting on the operator" (dropr:469). Built from
/// `crate::ui::tree::indicator`'s existing vocabulary and precedence rather
/// than a second one, and every other category carries no indicator at all,
/// so this returns `None` for them.
pub(super) fn category_indicator(
    app: &App,
    category: OverseerCategory,
    selected: bool,
) -> Option<Span<'static>> {
    if category != OverseerCategory::Inbox {
        return None;
    }
    let lit = crate::ui::overseer::inbox_actionable_count(app) > 0;
    let indicator = select(IndicatorState::with_status(lit.then_some(Status::Waiting)))?;
    Some(indicator::primary_span(
        Some(indicator),
        selected,
        app.started.elapsed(),
        1,
    ))
}
