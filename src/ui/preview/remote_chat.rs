use ratatui::text::{Line, Text};

use crate::{
    locale::t,
    model::Selection,
    overseer,
    ui::{App, theme::DEFAULT as THEME},
};

pub(super) fn render_local_discord(app: &App, index: usize) -> Option<(String, Text<'static>)> {
    let channels = &app.overseer_snapshot.discord_channels;
    let ids = crate::ui::overseer::ordered_channel_ids(channels);
    let channel_id = ids.get(index)?;
    let session =
        overseer::discord_channel_session_name(&app.config.tmux_session_prefix, channel_id);
    Some(app.cached_tmux(&session).map_or_else(
        || crate::ui::overseer::discord_channel_preview(app, index),
        |text| {
            (
                format!("Discord / {}", channels.display_label(channel_id)),
                text,
            )
        },
    ))
}

pub(super) fn render(app: &App, selection: Selection) -> Option<(String, Text<'static>)> {
    match selection {
        Selection::RemoteControlAi(host) => {
            let slot = app.hosts.get(host)?;
            let session = overseer::control_session_name(&app.config.tmux_session_prefix);
            let text = app.cached_tmux(&session).unwrap_or_else(|| {
                vec![Line::styled(
                    t(app.locale, "Control session not started."),
                    THEME.muted_style(),
                )]
                .into()
            });
            Some((format!("Control AI @{}", slot.label.name), text))
        }
        Selection::RemoteDiscordChannel { host, channel } => {
            let slot = app.hosts.get(host)?;
            let view = app.host_view(host)?;
            let ids = crate::ui::overseer::ordered_channel_ids(&view.discord_channels);
            let channel_id = ids.get(channel)?;
            let session =
                overseer::discord_channel_session_name(&app.config.tmux_session_prefix, channel_id);
            if let Some(text) = app.cached_tmux(&session) {
                return Some((
                    format!(
                        "Discord / {} @{}",
                        view.discord_channels.display_label(channel_id),
                        slot.label.name
                    ),
                    text,
                ));
            }
            let (title, text) = crate::ui::overseer::discord_channel_preview_from(
                app.locale,
                &view.discord_channels,
                channel,
            );
            Some((format!("{title} @{}", slot.label.name), text))
        }
        _ => None,
    }
}
