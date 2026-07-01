use std::process::Command;

use crate::config::{self, Config};

use super::super::{App, Mode, suspend_terminal};

impl App {
    /// Suspend the TUI and open `~/.robco/config.json` in the user's external
    /// editor, then reload the config so edits take effect without a restart.
    ///
    /// Like [`App::attach_session`], any failure is surfaced as an in-app message
    /// and MUST NOT propagate: a bubbling `Err` would unwind out of the event
    /// loop and exit robco (dropping the whole connection over ssh).
    pub(in crate::ui) fn open_settings_editor(&mut self) {
        self.force_redraw = true;
        match self.edit_config() {
            Ok(()) => match Config::load() {
                Ok(config) => {
                    self.config = config;
                    self.mode = Mode::Message("settings reloaded".to_string());
                }
                Err(err) => self.mode = Mode::Message(err.to_string()),
            },
            Err(err) => self.mode = Mode::Message(err.to_string()),
        }
    }

    /// Ensure the config file exists (seeding a default template when missing),
    /// then hand the terminal to `$EDITOR` for the duration of the edit.
    fn edit_config(&self) -> crate::Result<()> {
        let path = config::config_file_path()?;
        if !path.exists() {
            Config::default().save()?;
        }

        let editor = std::env::var("VISUAL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                std::env::var("EDITOR")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_else(|| "vi".to_string());

        // Run through `sh -c` so editor values carrying arguments (e.g.
        // `EDITOR="code -w"`) are honored. Single-quote the path and escape any
        // embedded single quotes so paths with spaces stay intact.
        let quoted = format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
        suspend_terminal(|| {
            let status = Command::new("sh")
                .arg("-c")
                .arg(format!("{editor} {quoted}"))
                .status()?;
            if !status.success() {
                return Err(crate::Error::Command {
                    context: "editor",
                    stderr: format!("exited with status {status}"),
                });
            }
            Ok(())
        })
    }
}
