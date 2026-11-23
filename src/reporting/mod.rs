//! Run reports: statistics, CSV, JSON and the console summary.

pub mod csv;
pub mod json;
pub mod statistics;

pub use statistics::{RunInfo, Summary};

use crate::errors::{Error, Result};
use crate::processing::FileResult;
use std::path::{Path, PathBuf};

/// Writes `report.csv` and `report.json` into `<output>/Reports`. Existing reports are replaced
/// (they are our own output). Returns the two paths.
pub fn write_reports(output: &Path, results: &[FileResult], info: &RunInfo, summary: &Summary) -> Result<(PathBuf, PathBuf)> {
    let dir = output.join("Reports");
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    let csv_path = dir.join("report.csv");
    let json_path = dir.join("report.json");
    csv::write(&csv_path, results)?;
    json::write(&json_path, results, info, summary)?;
    Ok((csv_path, json_path))
}
