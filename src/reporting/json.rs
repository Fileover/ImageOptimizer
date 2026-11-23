//! JSON report: run info + summary + every file result.

use super::statistics::{RunInfo, Summary};
use crate::errors::{Error, Result};
use crate::processing::FileResult;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct Report<'a> {
    run: &'a RunInfo,
    summary: &'a Summary,
    files: &'a [FileResult],
}

pub fn write(path: &Path, results: &[FileResult], info: &RunInfo, summary: &Summary) -> Result<()> {
    let text = serde_json::to_string_pretty(&Report { run: info, summary, files: results })
        .map_err(|e| Error::Invalid(format!("cannot serialize report: {e}")))?;
    std::fs::write(path, text).map_err(|e| Error::io(path, e))
}
