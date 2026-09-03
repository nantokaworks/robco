use super::*;

impl HostSlot {
    pub(in crate::ui) fn idle(label: HostLabel) -> Self {
        Self {
            label,
            snapshot: Arc::new(Mutex::new(HostSnapshot::default())),
            applied_generation: 0,
        }
    }

    pub(in crate::ui) fn connected(label: HostLabel) -> Self {
        Self::connected_with_chats(label, None, DiscordChannels::default(), true)
    }

    pub(in crate::ui) fn connected_with_chats(
        label: HostLabel,
        control_status: Option<Status>,
        discord_channels: DiscordChannels,
        daemon_alive: bool,
    ) -> Self {
        Self {
            label,
            snapshot: Arc::new(Mutex::new(HostSnapshot {
                control_status,
                discord_channels,
                daemon_alive,
                generation: 1,
                ..HostSnapshot::default()
            })),
            applied_generation: 0,
        }
    }

    pub(in crate::ui) fn failed(label: HostLabel, error: &str) -> Self {
        Self {
            label,
            snapshot: Arc::new(Mutex::new(HostSnapshot {
                error: Some(error.into()),
                generation: 1,
                ..HostSnapshot::default()
            })),
            applied_generation: 0,
        }
    }

    pub(in crate::ui) fn with_backend(label: HostLabel, backend: Arc<RemoteBackend>) -> Self {
        Self {
            label,
            snapshot: Arc::new(Mutex::new(HostSnapshot {
                backend: Some(backend),
                generation: 1,
                ..HostSnapshot::default()
            })),
            applied_generation: 0,
        }
    }

    pub(in crate::ui) fn replace_chats(&self, discord_channels: DiscordChannels) {
        let mut snapshot = self.snapshot.lock().unwrap();
        snapshot.discord_channels = discord_channels;
        snapshot.generation = snapshot.generation.wrapping_add(1);
    }

    pub(in crate::ui) fn replace_error(&self, error: Option<&str>) {
        let mut snapshot = self.snapshot.lock().unwrap();
        snapshot.error = error.map(str::to_string);
        snapshot.generation = snapshot.generation.wrapping_add(1);
    }
}

impl App {
    pub(in crate::ui) fn sync_remote_host_views(&mut self) {
        self.host_views = self.hosts.iter().map(HostSlot::snapshot_view).collect();
    }
}
