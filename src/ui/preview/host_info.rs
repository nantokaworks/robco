//! Read-only Info previews for the remote-host rows: the host header's
//! summary and the failed-host error detail. Split out of `preview.rs` to
//! keep that file under the source size limit; both render from the same
//! frame-stable `HostView` copy the tree rows draw from.

use ratatui::text::Text;

use crate::ui::{App, actions::remote_hosts::HostConnection};

fn state(connection: HostConnection) -> &'static str {
    match connection {
        HostConnection::Connecting => "connecting",
        HostConnection::Connected => "connected",
        HostConnection::Failed => "failed",
    }
}

/// The host header's Info: identity, connection state, what hangs under it,
/// and the daemon's liveness — plus the full error when one is recorded.
pub(super) fn header(app: &App, host: usize) -> Option<(String, Text<'static>)> {
    let (slot, view) = (app.hosts.get(host)?, app.host_view(host)?);
    let repos = app
        .registry
        .repos
        .iter()
        .filter(|repo| repo.host.as_ref() == Some(&slot.label))
        .count();
    let daemon = if view.daemon_alive { "alive" } else { "dead" };
    let mut text = format!(
        "host: {}\nssh: {}\nconnection: {}\nrepos: {repos}\ndaemon: {daemon}\ndaemon version: {}\nbinary: {}",
        slot.label.name,
        slot.label.ssh,
        state(view.connection),
        view.daemon_version.as_deref().unwrap_or("unknown"),
        view.binary_version.as_deref().unwrap_or("unknown")
    );
    if let Some(warning) = view.version_drift() {
        text.push('\n');
        text.push_str(&warning);
    }
    if let Some(error) = view.error.as_deref() {
        text.push_str("\nerror: ");
        text.push_str(&error.replace('\n', "\n       "));
    }
    Some((slot.label.name.clone(), text.into()))
}

/// The failed-host row's Info: the full multi-line error with the host's
/// identity above it.
pub(super) fn error(app: &App, host: usize) -> Option<(String, Text<'static>)> {
    let (slot, view) = (app.hosts.get(host)?, app.host_view(host)?);
    let error = view
        .error
        .as_deref()
        .unwrap_or_default()
        .replace('\n', "\n       ");
    let text = format!(
        "host: {}\nssh: {}\nconnection: {}\nerror: {error}",
        slot.label.name,
        slot.label.ssh,
        state(view.connection)
    );
    Some((slot.label.name.clone(), text.into()))
}
