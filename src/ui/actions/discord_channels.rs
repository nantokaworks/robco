//! Per-channel actions on Discord rows (dropr:417): resetting a channel
//! stuck at `Failed` back to `Idle`, and removing a retained channel record
//! entirely. Both read the state file fresh right before mutating rather
//! than writing back the cached `overseer_snapshot` copy, since the daemon's
//! Discord gateway can be writing the same file between refreshes.

use crate::{
    locale::{fmt, t},
    model::Selection,
    overseer::{self, discord_channels::DiscordChannels},
};

use super::super::{App, Mode};

impl App {
    /// Clears the selected channel's `Failed` status back to `Idle` and
    /// drops `last_error`. A channel that is not currently `Failed` (or no
    /// longer listed) gets an explicit message instead of a silent no-op,
    /// mirroring the global circuit-reset's "nothing to reset" message.
    pub(in crate::ui) fn reset_discord_channel_selected(&mut self, index: usize) {
        let Some(channel_id) = self.discord_channel_id_at(index) else {
            self.show_message(t(self.locale, "channel is no longer listed"));
            return;
        };
        let label = self
            .overseer_snapshot
            .discord_channels
            .display_label(&channel_id);
        let path = match discord_channels_path() {
            Ok(path) => path,
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        };
        let mut channels = match DiscordChannels::load(&path) {
            Ok(channels) => channels,
            Err(err) => {
                self.show_message(err);
                return;
            }
        };
        match channels.reset_channel(&path, &channel_id) {
            Ok(true) => {
                self.refresh_overseer_snapshot();
                self.show_message(fmt(self.locale, "reset {} to idle", &[&label]));
            }
            Ok(false) => {
                self.show_message(fmt(self.locale, "{} is not in a failed state", &[&label]));
            }
            Err(err) => self.show_message(err),
        }
    }

    /// Open the remove confirmation for the selected channel row. Removal is
    /// irreversible (the whole retained record — history included — is
    /// deleted), so it is gated the same way `ConfirmKillOrphan` gates an
    /// orphan session kill.
    pub(in crate::ui) fn confirm_remove_discord_channel_selected(&mut self, index: usize) {
        let Some(channel_id) = self.discord_channel_id_at(index) else {
            self.show_message(t(self.locale, "channel is no longer listed"));
            return;
        };
        let label = self
            .overseer_snapshot
            .discord_channels
            .display_label(&channel_id);
        self.mode = Mode::ConfirmRemoveDiscordChannel { channel_id, label };
    }

    /// Delete the retained record for `channel_id`, targeted by id (not
    /// index) since the confirm dialog may have been open across a refresh
    /// that resorted the row order.
    pub(in crate::ui) fn remove_discord_channel(&mut self, channel_id: &str, label: &str) {
        let path = match discord_channels_path() {
            Ok(path) => path,
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        };
        let mut channels = match DiscordChannels::load(&path) {
            Ok(channels) => channels,
            Err(err) => {
                self.show_message(err);
                return;
            }
        };
        match channels.remove_channel(&path, channel_id) {
            Ok(true) => {
                self.refresh_overseer_snapshot();
                self.clamp_selection();
                self.show_message(fmt(self.locale, "removed channel {}", &[label]));
            }
            Ok(false) => {
                self.show_message(t(self.locale, "channel is no longer listed"));
            }
            Err(err) => self.show_message(err),
        }
    }

    fn discord_channel_id_at(&self, index: usize) -> Option<String> {
        if !matches!(self.selected_item(), Some(Selection::DiscordChannel(_))) {
            return None;
        }
        crate::ui::overseer::ordered_channel_ids(&self.overseer_snapshot.discord_channels)
            .get(index)
            .cloned()
    }
}

fn discord_channels_path() -> crate::Result<std::path::PathBuf> {
    Ok(overseer::discord_ops_dir()?.join("channels.json"))
}

#[cfg(test)]
#[path = "discord_channels_tests.rs"]
mod tests;
