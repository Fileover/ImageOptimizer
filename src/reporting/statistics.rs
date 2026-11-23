//! Aggregate statistics for a run.

use crate::processing::{FileResult, Status};
use serde::Serialize;
use std::collections::BTreeMap;

/// Static information about the run (settings, environment, codec versions).
#[derive(Serialize, Clone)]
pub struct RunInfo {
    pub application: String,
    pub version: String,
    pub started_unix: u64,
    pub wall_ms: f64,
    pub jobs: usize,
    pub threads: usize,
    pub logical_cpus: usize,
    pub oversubscribed: bool,
    pub codecs: BTreeMap<String, String>,
    pub settings: crate::config::Settings,
}

#[derive(Serialize, Default, Clone)]
pub struct GroupStats {
    pub files: usize,
    pub source_bytes: u64,
    pub output_bytes: u64,
    pub encode_ms: f64,
}

#[derive(Serialize, Default, Clone)]
pub struct Summary {
    pub files_seen: usize,
    pub written: usize,
    pub kept_original: usize,
    pub dry_run: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Totals over files that produced an output (written + kept-original).
    pub source_bytes: u64,
    pub output_bytes: u64,
    pub saved_bytes: i64,
    pub saved_percent: f64,
    pub larger_than_source: usize,
    pub load_ms: f64,
    pub decode_ms: f64,
    pub encode_ms: f64,
    pub verify_ms: f64,
    pub write_ms: f64,
    pub cpu_total_ms: f64,
    pub throughput_mib_s: f64,
    pub by_conversion: BTreeMap<String, GroupStats>,
}

impl Summary {
    pub fn compute(results: &[FileResult], wall_ms: f64) -> Summary {
        let mut s = Summary { files_seen: results.len(), ..Default::default() };
        for r in results {
            match r.status {
                Status::Written => s.written += 1,
                Status::KeptOriginal => s.kept_original += 1,
                Status::DryRun => s.dry_run += 1,
                Status::Skipped => s.skipped += 1,
                Status::Failed => s.failed += 1,
            }
            s.load_ms += r.timings.load_ms;
            s.decode_ms += r.timings.decode_ms;
            s.encode_ms += r.timings.encode_ms;
            s.verify_ms += r.timings.verify_ms;
            s.write_ms += r.timings.write_ms;
            s.cpu_total_ms += r.timings.total_ms;
            if matches!(r.status, Status::Written | Status::KeptOriginal) {
                s.source_bytes += r.source_bytes;
                s.output_bytes += r.output_bytes;
                if r.output_bytes > r.source_bytes {
                    s.larger_than_source += 1;
                }
                let key = format!("{} -> {}", r.source_format.unwrap_or("?"), r.target_format.unwrap_or("?"));
                let g = s.by_conversion.entry(key).or_default();
                g.files += 1;
                g.source_bytes += r.source_bytes;
                g.output_bytes += r.output_bytes;
                g.encode_ms += r.timings.encode_ms;
            }
        }
        s.saved_bytes = s.source_bytes as i64 - s.output_bytes as i64;
        s.saved_percent = if s.source_bytes == 0 { 0.0 } else { s.saved_bytes as f64 / s.source_bytes as f64 * 100.0 };
        if wall_ms > 0.0 {
            s.throughput_mib_s = (s.source_bytes as f64 / (1024.0 * 1024.0)) / (wall_ms / 1000.0);
        }
        s
    }

    pub fn exit_code(&self) -> i32 {
        if self.failed > 0 {
            1
        } else {
            0
        }
    }
}
