use crate::model::Selection;

use super::super::App;

impl App {
    pub(super) fn expand_selected_tree_item(&mut self) {
        match self.selected_item() {
            Some(Selection::Overseer) => self.set_overseer_collapsed(false),
            Some(Selection::OverseerCategory(category)) => {
                self.set_overseer_category_expanded(category, true);
            }
            Some(Selection::Repo(repo)) => {
                if let Some(expanded) = self.expanded.get_mut(repo) {
                    *expanded = true;
                }
            }
            Some(Selection::OtherHeader) => self.set_other_collapsed(false),
            Some(Selection::OrphanHeader) => self.set_orphans_collapsed(false),
            _ => {}
        }
    }

    pub(super) fn collapse_selected_tree_item(&mut self) {
        match self.selected_item() {
            Some(Selection::Overseer) => self.set_overseer_collapsed(true),
            Some(Selection::OverseerCategory(category)) => {
                self.set_overseer_category_expanded(category, false);
            }
            Some(Selection::Repo(repo)) => {
                if let Some(expanded) = self.expanded.get_mut(repo) {
                    *expanded = false;
                }
            }
            Some(Selection::OtherHeader) => self.set_other_collapsed(true),
            Some(Selection::OrphanHeader) => self.set_orphans_collapsed(true),
            _ => {}
        }
    }

    pub(super) fn toggle_selected_tree_header(&mut self, selection: Selection) -> bool {
        match selection {
            Selection::Overseer => self.set_overseer_collapsed(!self.overseer_collapsed),
            Selection::OverseerCategory(category) => self.toggle_overseer_category(category),
            Selection::OtherHeader => self.set_other_collapsed(!self.other_collapsed),
            Selection::OrphanHeader => self.set_orphans_collapsed(!self.orphans_collapsed),
            _ => return false,
        }
        true
    }
}
