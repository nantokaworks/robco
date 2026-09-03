use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::{
    config::Config,
    model::{HostLabel, Selection},
    registry::Registry,
    ui::actions::remote_hosts::HostSlot,
};

fn failed_host_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = false;
    app.orphans.clear();
    app.hosts = vec![HostSlot::failed(
        HostLabel {
            name: "Production".into(),
            ssh: "ops@prod.example".into(),
        },
        "ssh handshake failed\nconnection timed out",
    )];
    app.sync_remote_host_views();
    app
}

#[test]
fn failed_host_info_contains_the_full_error() {
    let app = failed_host_app();
    let selection = Selection::RemoteHostError(0);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal
        .draw(|frame| draw(frame, &app, Some(selection)))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let rendered = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer.cell((x, y)).unwrap().symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("host: Production"));
    assert!(rendered.contains("ssh: ops@prod.example"));
    assert!(rendered.contains("connection: failed"));
    assert!(rendered.contains("ssh handshake failed"));
    assert!(rendered.contains("connection timed out"));
}

#[test]
fn failed_host_action_keys_are_inert() {
    let mut app = failed_host_app();
    for code in [KeyCode::Enter, KeyCode::Char('y'), KeyCode::Char('d')] {
        app.force_redraw = false;
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.selected_item(), Some(Selection::RemoteHostError(0)));
        assert!(app.message.is_none());
        assert!(!app.force_redraw, "inert keys must not attempt an attach");
    }
}
