//! Which client an ephemeral session runs.

use crate::config::{Config, Profile};

/// Resolves the profile an ephemeral judgment or review session runs under.
///
/// `selected` is the surface's own profile setting. When it is set the profile
/// must exist — a named profile that is missing is a configuration error, not a
/// reason to fall back to the default client. When it is unset the default
/// program stands in, so a daemon with no profiles configured still has a
/// session to run. A profile that names a `backend` borrows that backend's
/// program, which is how one client can drive another's binary.
pub(crate) fn session_profile(config: &Config, selected: Option<&String>) -> Option<Profile> {
    let name = selected.unwrap_or(&config.default_program);
    config
        .profiles
        .iter()
        .find(|profile| &profile.name == name)
        .cloned()
        .or_else(|| {
            selected.is_none().then(|| Profile {
                name: name.clone(),
                program: config.default_program_command(),
                autonomous_args: Vec::new(),
                model: None,
                backend: None,
            })
        })
        .map(|mut profile| {
            if let Some(backend) = profile.backend.as_deref()
                && let Some(program) = config
                    .profiles
                    .iter()
                    .find(|candidate| candidate.name == backend)
                    .map(|candidate| candidate.program.clone())
            {
                profile.program = program;
            }
            profile
        })
}
