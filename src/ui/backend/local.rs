use std::path::{Path, PathBuf};

use ratatui::text::Text;

use crate::{config::Config, registry::Registry};

use super::Backend;
use crate::ui::actions::{
    background_refresh::{StatusResult, capture_status},
    discovery_capture::{DiscoveryResult, capture_discovery},
    overseer_refresh::{ControlWatch, OverseerResult, capture_overseer},
    preview_capture::{PreviewCapture, cached_diff, cached_tmux},
};

pub(in crate::ui) struct LocalBackend;

impl Backend for LocalBackend {
    fn capture_status(
        &self,
        registry: Registry,
        config: &Config,
        control_watch: &ControlWatch,
    ) -> StatusResult {
        capture_status(registry, config, control_watch, capture_overseer)
    }

    fn capture_overseer(
        &self,
        registry: &Registry,
        config: &Config,
        control_watch: &ControlWatch,
    ) -> OverseerResult {
        capture_overseer(registry, config, control_watch)
    }

    fn capture_discovery(
        &self,
        registry: Registry,
        config: Config,
        roots: Vec<PathBuf>,
        reload_overlay: bool,
    ) -> DiscoveryResult {
        capture_discovery(registry, config, roots, reload_overlay)
    }

    fn cached_tmux(
        &self,
        preview_capture: &PreviewCapture,
        session: &str,
    ) -> Option<Text<'static>> {
        cached_tmux(preview_capture, session)
    }

    fn cached_diff(&self, preview_capture: &PreviewCapture, path: &Path) -> Option<Text<'static>> {
        cached_diff(preview_capture, path)
    }
}
