use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use fd_lock::RwLock;

use super::process_alive;
use crate::Result;

pub(crate) fn append_jsonl(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub(crate) struct PidGuard {
    path: PathBuf,
}

impl PidGuard {
    pub(crate) fn acquire(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("pid.lock"))?;
        let mut lock = RwLock::new(lock_file);
        let _guard = lock.write()?;
        if let Ok(raw) = fs::read_to_string(&path)
            && raw.trim().parse::<u32>().ok().is_some_and(process_alive)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "overseer daemon is already running",
            )
            .into());
        }
        let _ = fs::remove_file(&path);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .write_all(std::process::id().to_string().as_bytes())?;
        Ok(Self { path })
    }
}

impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
