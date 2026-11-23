//! ImageOptimizer - one Rust application integrating libjxl, libwebp, jpegoptim (MozJPEG)
//! and OxiPNG (Zopfli) through native APIs. See ARCHITECTURE.md.

pub mod app;
pub mod cli;
pub mod config;
pub mod engines;
pub mod errors;
pub mod formats;
pub mod image;
pub mod logging;
pub mod processing;
pub mod reporting;
pub mod utils;
