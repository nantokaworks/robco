use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub(crate) fn resolve_program_impl(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() || name.contains(std::path::MAIN_SEPARATOR) {
        return is_executable(path).then(|| path.to_path_buf());
    }
    let mut dirs: Vec<_> = env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect())
        .unwrap_or_default();
    let home = dirs::home_dir();
    dirs.extend(
        [
            home.as_ref().map(|path| path.join(".local/bin")),
            Some(PathBuf::from("/usr/local/bin")),
            Some(PathBuf::from("/opt/homebrew/bin")),
            home.as_ref().map(|path| path.join(".cargo/bin")),
            home.as_ref().map(|path| path.join(".bun/bin")),
            home.as_ref().map(|path| path.join(".volta/bin")),
            Some(PathBuf::from("/usr/bin")),
            Some(PathBuf::from("/bin")),
        ]
        .into_iter()
        .flatten(),
    );
    resolve_in_dirs(dirs, name)
}

fn resolve_in_dirs(dirs: impl IntoIterator<Item = PathBuf>, name: &str) -> Option<PathBuf> {
    dirs.into_iter()
        .map(|dir| dir.canonicalize().unwrap_or(dir).join(name))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn write_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, "#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn earlier_directory_wins() {
        let temp = tempfile::tempdir().unwrap();
        let earlier = temp.path().join("earlier");
        let later = temp.path().join("later");
        fs::create_dir_all(&earlier).unwrap();
        fs::create_dir_all(&later).unwrap();
        write_executable(&earlier.join("agent"));
        write_executable(&later.join("agent"));

        assert_eq!(
            resolve_in_dirs([earlier.clone(), later], "agent"),
            Some(earlier.canonicalize().unwrap().join("agent"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_executable_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("agent");
        fs::write(&path, "not executable").unwrap();

        assert_eq!(resolve_in_dirs([temp.path().to_path_buf()], "agent"), None);
        assert_eq!(resolve_program_impl(path.to_str().unwrap()), None);
    }

    #[test]
    fn absent_bare_name_is_not_resolved() {
        let temp = tempfile::tempdir().unwrap();

        assert_eq!(
            resolve_in_dirs([temp.path().to_path_buf()], "missing-agent"),
            None
        );
    }

    #[test]
    fn resolves_executable_explicit_path_and_rejects_missing_name() {
        assert_eq!(resolve_program_impl("robco-nonexistent-binary-xyzzy"), None);
        #[cfg(unix)]
        assert_eq!(
            resolve_program_impl("/bin/sh"),
            Some(PathBuf::from("/bin/sh"))
        );
    }
}
