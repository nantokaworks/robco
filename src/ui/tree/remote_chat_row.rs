use ratatui::text::{Line, Span};

use crate::{
    model::{Selection, Status},
    overseer::discord_channels::ChannelAgentStatus,
    ui::App,
};

use super::{IndicatorState, THEME, indicator, select};

pub(super) fn build(
    app: &App,
    selection: Selection,
    selected: bool,
    marker: &str,
) -> Option<Line<'static>> {
    let (host, label, status) = match selection {
        Selection::RemoteControlAi(host) => {
            let view = app.host_view(host)?;
            (host, "Control AI".to_string(), view.control_status)
        }
        Selection::RemoteDiscordChannel { host, channel } => {
            let view = app.host_view(host)?;
            let id = crate::ui::overseer::ordered_channel_ids(&view.discord_channels)
                .get(channel)?
                .clone();
            let agent = view.discord_channels.channels.get(&id)?;
            let status = match agent.status {
                ChannelAgentStatus::Running => Status::Running,
                ChannelAgentStatus::Idle => Status::Idle,
                ChannelAgentStatus::Failed => Status::Dead,
            };
            (host, view.discord_channels.display_label(&id), Some(status))
        }
        _ => return None,
    };
    let slot = app.hosts.get(host)?;
    let style = if selected {
        THEME.selection_style()
    } else {
        THEME.accent_style()
    };
    let primary = select(IndicatorState::with_status(status));
    Some(Line::from(vec![
        Span::styled(format!("{marker}   {label} @{}  ", slot.label.name), style),
        indicator::primary_span(primary, selected, app.started.elapsed(), 1),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Config, model::HostLabel, overseer::discord_channels::DiscordChannels,
        registry::Registry, ui::actions::remote_hosts::HostSlot,
    };

    #[test]
    fn rows_include_labels_and_host_name() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        let channels: DiscordChannels = serde_json::from_value(serde_json::json!({"channels": {
            "42": {"first_seen_at":"2025-01-01T00:00:00Z","last_active_at":"2025-01-01T00:00:00Z",
                "turn_count":1,"status":"idle","last_error":null,"channel_name":"ops"}
        }}))
        .unwrap();
        app.hosts = vec![HostSlot::connected_with_chats(
            HostLabel {
                name: "Prod".into(),
                ssh: "prod".into(),
            },
            Some(Status::Waiting),
            channels,
            true,
        )];
        app.sync_remote_host_views();

        let control = build(&app, Selection::RemoteControlAi(0), false, " ").unwrap();
        let discord = build(
            &app,
            Selection::RemoteDiscordChannel {
                host: 0,
                channel: 0,
            },
            false,
            " ",
        )
        .unwrap();
        assert!(control.to_string().contains("Control AI @Prod"));
        assert!(discord.to_string().contains("#ops @Prod"));
    }
}
