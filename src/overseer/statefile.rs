//! On-disk discipline shared by the Overseer's state files.
//!
//! Every writer here is racing the daemon: the poll loop rewrites `ledger.json`
//! and appends to `inbox.jsonl` while a TUI or CLI invocation may be writing the
//! same directory. Two rules keep that safe, and both belong to the file rather
//! than to any one caller — write through a temporary file so a crash can never
//! leave a half-written state file behind, and take an advisory lock on a
//! sibling `.lock` so two writers cannot interleave.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use fd_lock::RwLock;

use crate::Result;

/// Replace `path` with `contents` atomically. The temporary file is removed on
/// failure so a retry does not inherit a partial write.
pub(super) fn atomic_replace(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(".tmp");
    let temp_path = PathBuf::from(temp_name);
    let result = (|| -> std::io::Result<()> {
        let mut temp = File::create(&temp_path)?;
        temp.write_all(contents)?;
        temp.sync_all()?;
        fs::rename(&temp_path, path)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp_path);
    }
    result.map_err(Into::into)
}

/// Run `operation` while holding the exclusive advisory lock for `path`. The
/// lock lives on `path.lock` rather than on the file itself so it survives the
/// rename [`atomic_replace`] performs.
pub(super) fn with_lock<T>(path: &Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))?;
    let mut lock = RwLock::new(lock_file);
    let _guard = lock.write()?;
    operation()
}
