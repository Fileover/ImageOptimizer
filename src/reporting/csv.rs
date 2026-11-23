//! CSV report: one row per input file, invariant raw numbers (bytes, milliseconds).

use crate::errors::{Error, Result};
use crate::processing::FileResult;
use std::path::Path;

const HEADER: [&str; 21] = [
    "source",
    "output",
    "source_format",
    "target_format",
    "status",
    "mode",
    "source_bytes",
    "output_bytes",
    "saved_bytes",
    "saved_percent",
    "width",
    "height",
    "verification",
    "load_ms",
    "decode_ms",
    "encode_ms",
    "verify_ms",
    "write_ms",
    "total_ms",
    "codec_settings",
    "notes_and_errors",
];

pub fn write(path: &Path, results: &[FileResult]) -> Result<()> {
    let mut w = ::csv::Writer::from_path(path).map_err(|e| Error::Invalid(format!("cannot write {}: {e}", path.display())))?;
    let err = |e: ::csv::Error| Error::Invalid(format!("cannot write {}: {e}", path.display()));
    w.write_record(HEADER).map_err(err)?;
    for r in results {
        let saved = r.saved_bytes();
        let pct = if r.source_bytes > 0 && r.output_bytes > 0 { saved as f64 / r.source_bytes as f64 * 100.0 } else { 0.0 };
        let mut msg = r.notes.join(" | ");
        if let Some(e) = &r.error {
            if !msg.is_empty() {
                msg.push_str(" | ");
            }
            msg.push_str("ERROR: ");
            msg.push_str(e);
        }
        let rec = [
            r.source.to_string_lossy().into_owned(),
            r.output.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            r.source_format.unwrap_or("").to_string(),
            r.target_format.unwrap_or("").to_string(),
            r.status.as_str().to_string(),
            r.mode.to_string(),
            r.source_bytes.to_string(),
            r.output_bytes.to_string(),
            saved.to_string(),
            format!("{pct:.3}"),
            r.width.map(|v| v.to_string()).unwrap_or_default(),
            r.height.map(|v| v.to_string()).unwrap_or_default(),
            r.verification.as_str().to_string(),
            format!("{:.3}", r.timings.load_ms),
            format!("{:.3}", r.timings.decode_ms),
            format!("{:.3}", r.timings.encode_ms),
            format!("{:.3}", r.timings.verify_ms),
            format!("{:.3}", r.timings.write_ms),
            format!("{:.3}", r.timings.total_ms),
            r.codec_settings.clone(),
            msg,
        ];
        w.write_record(&rec).map_err(err)?;
    }
    w.flush().map_err(|e| Error::io(path, e))?;
    Ok(())
}
