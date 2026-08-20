//! `robco decisions compact`: quarantine unparseable decision-log
//! lines to a sidecar file, keeping every valid line intact and in order.

use crate::{
    Result,
    overseer::logging::{self, CompactionReport},
};

pub(super) fn compact_decisions(dry_run: bool) -> Result<()> {
    let report: CompactionReport = logging::compact(dry_run)?;
    if dry_run {
        println!(
            "dry run: {} line(s) would be kept, {} would be quarantined to {}",
            report.kept,
            report.quarantined,
            report.sidecar_path.display()
        );
        return Ok(());
    }
    if report.quarantined == 0 {
        println!("{} line(s) kept, nothing to quarantine", report.kept);
        return Ok(());
    }
    println!(
        "{} line(s) kept, {} quarantined to {}",
        report.kept,
        report.quarantined,
        report.sidecar_path.display()
    );
    Ok(())
}
