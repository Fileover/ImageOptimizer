//! Per-file processing and scheduling.

pub mod pipeline;
pub mod scheduler;

pub use pipeline::{process_file, FileResult, Status, Timings};
