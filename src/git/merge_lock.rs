//! A cross-process, per-repository merge lock.
//!
//! The merge sequence races on the target repository's base branch and working
//! tree, so only one may run against a repository at a time. The TUI already
//! serialises its own merges in memory, but the MCP server and the TUI are
//! separate processes and neither can see the other's state — the lock is what
//! makes that guarantee hold across both.
//!
//! It is an advisory `flock`, so the kernel drops it when the holder exits.
//! There is no stale lock to reap and no pid to second-guess: a robco that
//! crashed mid-merge leaves the next one free to run, which is the same
//! position the repository would be in either way.

use std::{
    fs::{self, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use fd_lock::RwLock;

use crate::{Error, Result};

/// Runs `f` while holding the repository's merge lock. Refuses rather than
/// waits: the callers are a UI banner and a tool call, and both would rather
/// tell their reader that a merge is already running than block for however
/// long the other one takes.
pub fn with_merge_lock<T>(repo: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let mut lock = open(repo)?;
    let _guard = lock.try_write().map_err(|error| busy(repo, &error))?;
    f()
}

/// Runs `f` while holding the repository's merge lock, or returns `Ok(None)`
/// when another process already holds it.
///
/// The difference from [`with_merge_lock`] is who the contention is reported
/// to. A merge has a reader waiting on its result, so being refused is an
/// error worth returning. The Overseer daemon has none: it is mid-pass over
/// every repository it watches, and a merge running in one of them is an
/// ordinary thing to find rather than a cleanup that failed. Losing the race
/// leaves the work for the next pass, so the caller wants to *know* it lost
/// rather than to be handed an error it would only have to recognise and
/// discard.
pub fn with_merge_lock_if_free<T>(repo: &Path, f: impl FnOnce() -> Result<T>) -> Result<Option<T>> {
    run_if_free(&mut open(repo)?, repo, f)
}

fn run_if_free<T>(
    lock: &mut RwLock<fs::File>,
    repo: &Path,
    f: impl FnOnce() -> Result<T>,
) -> Result<Option<T>> {
    match lock.try_write() {
        Ok(_guard) => f().map(Some),
        // Only contention reads as "not now". Anything else is a lock the
        // caller could not evaluate at all, which is not the same as one that
        // is busy and must not be retried as though it were.
        Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(busy(repo, &error)),
    }
}

fn open(repo: &Path) -> Result<RwLock<fs::File>> {
    let path = lock_path(repo)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        // The file is only ever a lock handle; its contents carry nothing.
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)?;
    Ok(RwLock::new(file))
}

fn busy(repo: &Path, error: &std::io::Error) -> Error {
    if matches!(error.kind(), ErrorKind::WouldBlock) {
        return Error::Command {
            context: "merge lock",
            stderr: format!(
                "another robco process is already merging in {}",
                repo.display()
            ),
        };
    }
    Error::Command {
        context: "merge lock",
        stderr: error.to_string(),
    }
}

fn lock_path(repo: &Path) -> Result<PathBuf> {
    Ok(crate::config::robco_dir()?
        .join("merge-locks")
        .join(lock_file_name(repo)))
}

/// A readable stem so an operator listing the directory can tell which
/// repository a lock belongs to, plus a fingerprint of the full path so two
/// repositories with the same directory name cannot share one lock.
fn lock_file_name(repo: &Path) -> String {
    let full = repo.display().to_string();
    let stem: String = repo
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    format!("{stem}-{:016x}.lock", fingerprint(&full))
}

/// FNV-1a, written out rather than taken from `DefaultHasher` because the value
/// lands in a filename that two separately built robco binaries have to agree
/// on, and `DefaultHasher`'s output is explicitly not stable across builds.
fn fingerprint(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_repository_always_maps_to_the_same_lock_file() {
        let repo = Path::new("/Users/someone/code/robco");
        assert_eq!(lock_file_name(repo), lock_file_name(repo));
        assert!(lock_file_name(repo).starts_with("robco-"));
        assert!(lock_file_name(repo).ends_with(".lock"));
    }

    /// Two checkouts of the same project under different parents share a
    /// directory name. Sharing a lock would make a merge in one refuse because
    /// of a merge in the other.
    #[test]
    fn repositories_sharing_a_directory_name_do_not_share_a_lock() {
        assert_ne!(
            lock_file_name(Path::new("/one/robco")),
            lock_file_name(Path::new("/two/robco"))
        );
    }

    #[test]
    fn path_separators_never_reach_the_file_name() {
        let name = lock_file_name(Path::new("/tmp/we ird/na/me.git"));
        assert!(!name.contains('/'));
        assert!(name.starts_with("me-git-"));
    }

    #[test]
    fn a_second_holder_is_refused_rather_than_queued() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("repo.lock");
        let held = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut held = RwLock::new(held);
        let _guard = held.write().unwrap();

        let contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut contender = RwLock::new(contender);
        let error = busy(
            Path::new("/repo"),
            &contender.try_write().map(|_| ()).unwrap_err(),
        );

        assert!(error.to_string().contains("already merging in /repo"));
    }

    /// A lock nobody holds runs the work and hands back what it returned.
    #[test]
    fn a_free_lock_runs_the_work() {
        let temp = tempfile::tempdir().unwrap();
        let mut lock = lock_over(&temp.path().join("repo.lock"));

        let ran = run_if_free(&mut lock, Path::new("/repo"), || Ok("done")).unwrap();

        assert_eq!(ran, Some("done"));
    }

    /// The daemon's case: a cleanup that arrives while a merge is running is
    /// told the lock was busy rather than handed an error. The work must not
    /// run — half of it against a repository someone else is merging is worse
    /// than none of it, and the next pass will retry.
    #[test]
    fn a_held_lock_reports_contention_without_running_the_work() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("repo.lock");
        let mut held = lock_over(&path);
        let _guard = held.write().unwrap();
        let mut contender = lock_over(&path);
        let mut ran = false;

        let outcome = run_if_free(&mut contender, Path::new("/repo"), || {
            ran = true;
            Ok(())
        })
        .unwrap();

        assert_eq!(outcome, None);
        assert!(!ran, "the work ran while another holder had the lock");
    }

    fn lock_over(path: &Path) -> RwLock<fs::File> {
        RwLock::new(
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path)
                .unwrap(),
        )
    }
}
