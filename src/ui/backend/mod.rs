mod local;

use std::path::{Path, PathBuf};

use ratatui::text::Text;

use crate::{config::Config, registry::Registry};

use super::actions::{
    background_refresh::StatusResult,
    discovery_capture::DiscoveryResult,
    overseer_refresh::{ControlWatch, OverseerResult},
    preview_capture::PreviewCapture,
};

pub(in crate::ui) use local::LocalBackend;

/// Read-only host state used by the TUI.
///
/// A remote implementation must replace local process checks, including
/// `daemon_pid_alive()` in overseer capture and
/// `status::proc::ProcSnapshot::capture()` in status capture.
pub(in crate::ui) trait Backend: Send + Sync {
    fn capture_status(
        &self,
        registry: Registry,
        config: &Config,
        control_watch: &ControlWatch,
    ) -> StatusResult;

    fn capture_overseer(
        &self,
        registry: &Registry,
        config: &Config,
        control_watch: &ControlWatch,
    ) -> OverseerResult;

    fn capture_discovery(
        &self,
        registry: Registry,
        config: Config,
        roots: Vec<PathBuf>,
        reload_overlay: bool,
    ) -> DiscoveryResult;

    fn cached_tmux(&self, preview_capture: &PreviewCapture, session: &str)
    -> Option<Text<'static>>;

    fn cached_diff(&self, preview_capture: &PreviewCapture, path: &Path) -> Option<Text<'static>>;
}
