//! Quarantines unparseable decision-log lines to a sidecar file, keeping
//! every valid line byte-identical and in order, and staying safe to run
//! while the daemon is actively appending.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use crate::Result;

use super::DecisionEntry;

/// Result of a decision-log compaction pass.
pub struct CompactionReport {
    pub kept: usize,
    pub quarantined: usize,
    pub sidecar_path: PathBuf,
}

/// Quarantine unparseable lines out of the decision log into a sidecar file
/// next to it, keeping every valid line byte-identical and in order. `dry_run`
/// reports the counts without rewriting anything.
pub fn compact(dry_run: bool) -> Result<CompactionReport> {
    compact_at(&super::super::decision_log_path()?, dry_run)
}

/// Bounds the compare-and-swap retry below. Chosen generously: real decision
/// appends are one small `write_all` apiece, seconds to minutes apart, so
/// converging on a stable snapshot takes one or two attempts in practice —
/// this only guards against pathological, sustained concurrent write bursts.
const MAX_COMPACT_ATTEMPTS: usize = 200;

pub(crate) fn compact_at(path: &Path, dry_run: bool) -> Result<CompactionReport> {
    #[cfg(test)]
    super::refuse_the_operators_real_home(path);
    let sidecar_path = quarantine_sidecar_path(path);
    if dry_run {
        let content = read_or_empty(path)?;
        let (_, _, kept_count, quarantined_count) = split_lines(&content);
        return Ok(CompactionReport {
            kept: kept_count,
            quarantined: quarantined_count,
            sidecar_path,
        });
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("decisions.jsonl");
    let tmp_path = dir.join(format!(".{file_name}.compact.tmp"));
    for _ in 0..MAX_COMPACT_ATTEMPTS {
        let content = read_or_empty(path)?;
        let (kept, quarantined, kept_count, quarantined_count) = split_lines(&content);
        if quarantined_count == 0 {
            return Ok(CompactionReport {
                kept: kept_count,
                quarantined: 0,
                sidecar_path,
            });
        }
        fs::write(&tmp_path, &kept)?;
        // `append_to` only ever grows the file (single `write_all` per call, no
        // rewrite of earlier bytes), so a length match here means `content` is
        // still exactly what's on disk: nothing appended between the read above
        // and this check. Appenders open the file fresh by path on every call,
        // so once the rename below lands every subsequent append targets the
        // compacted file — this check is what makes the swap lossless instead
        // of merely atomic.
        let unchanged = fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            == content.len() as u64;
        if !unchanged {
            let _ = fs::remove_file(&tmp_path);
            continue;
        }
        fs::rename(&tmp_path, path)?;
        let mut sidecar = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sidecar_path)?;
        sidecar.write_all(&quarantined)?;
        return Ok(CompactionReport {
            kept: kept_count,
            quarantined: quarantined_count,
            sidecar_path,
        });
    }
    Err(std::io::Error::other(
        "decision log kept growing during compaction; retry once appends settle",
    )
    .into())
}

fn quarantine_sidecar_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".quarantine");
    PathBuf::from(name)
}

fn read_or_empty(path: &Path) -> Result<Vec<u8>> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

/// Split raw log bytes into kept and quarantined line groups, preserving kept
/// lines byte-for-byte and in order. Blank lines are kept but uncounted, same
/// as `corrupt_line_count_at`. A trailing fragment with no `\n` (an in-flight
/// write) is preserved verbatim and left unclassified for the next run.
fn split_lines(content: &[u8]) -> (Vec<u8>, Vec<u8>, usize, usize) {
    let mut kept = Vec::with_capacity(content.len());
    let mut quarantined = Vec::new();
    let mut kept_count = 0usize;
    let mut quarantined_count = 0usize;
    let mut start = 0usize;
    while let Some(relative_newline) = content[start..].iter().position(|&byte| byte == b'\n') {
        let end = start + relative_newline;
        let line = &content[start..=end];
        let text = &content[start..end];
        match std::str::from_utf8(text) {
            Ok(text) if text.trim().is_empty() => kept.extend_from_slice(line),
            Ok(text) if serde_json::from_str::<DecisionEntry>(text).is_ok() => {
                kept.extend_from_slice(line);
                kept_count += 1;
            }
            _ => {
                quarantined.extend_from_slice(line);
                quarantined_count += 1;
            }
        }
        start = end + 1;
    }
    if start < content.len() {
        kept.extend_from_slice(&content[start..]);
    }
    (kept, quarantined, kept_count, quarantined_count)
}

#[cfg(test)]
#[path = "compact_tests.rs"]
mod tests;
