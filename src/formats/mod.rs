//! Format layer: encoding profiles (upstream defaults, strict lossless, explicit lossy),
//! raster preparation rules and result verification for each output format.
//! Engines (`crate::engines`) do the actual coding.

pub mod jpeg;
pub mod jpegxl;
pub mod png;
pub mod webp;

use crate::config::{Mode, Settings};
use crate::image::decoder::DecodeLimits;
use serde::Serialize;

/// Per-run context handed to every format function.
pub struct Ctx<'a> {
    pub settings: &'a Settings,
    /// Codec-internal threads for this file (already coordinated with `jobs`).
    pub threads: i32,
}

impl Ctx<'_> {
    /// True only when `--lossless` was requested globally.
    pub fn strict_lossless(&self) -> bool {
        self.settings.mode == Mode::Lossless
    }
    /// True only when `--lossy` was requested globally.
    pub fn force_lossy(&self) -> bool {
        self.settings.mode == Mode::Lossy
    }
    pub fn limits(&self) -> DecodeLimits {
        DecodeLimits { max_pixels: self.settings.max_pixels, threads: self.threads }
    }
}

/// Result of an encode/optimize step.
pub struct Encoded {
    pub bytes: Vec<u8>,
    /// True when the produced bitstream is mathematically lossless (or lossless JPEG transcode).
    pub lossless: bool,
    /// Human readable description of the exact codec settings used (goes into the report).
    pub settings: String,
    /// Limitations / warnings (dropped alpha, metadata that could not be carried, ...).
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Verification {
    #[serde(rename = "not-run")]
    NotRun,
    /// Decoded samples of source and output are identical (including hidden RGB under alpha 0).
    #[serde(rename = "pixel-identical")]
    PixelIdentical,
    /// Visible pixels and alpha channel are identical; hidden RGB under alpha 0 was optimized (--optimize-alpha).
    #[serde(rename = "visible-pixels-identical")]
    VisiblePixelsIdentical,
    /// Output is a byte-for-byte copy of the source (nothing was re-encoded).
    #[serde(rename = "byte-identical-copy")]
    ByteIdentical,
    /// Every quantized DCT coefficient (and the quantization tables) are identical.
    #[serde(rename = "dct-coefficients-identical")]
    CoefficientsIdentical,
    /// libjxl reconstructed the original JPEG byte-for-byte.
    #[serde(rename = "jpeg-bitstream-identical")]
    BitstreamIdentical,
    /// Lossy output: it decodes and has the right dimensions; pixels are NOT compared.
    #[serde(rename = "decodable-lossy-not-compared")]
    DecodableOnly,
}

impl Verification {
    pub fn as_str(self) -> &'static str {
        match self {
            Verification::NotRun => "not-run",
            Verification::PixelIdentical => "pixel-identical",
            Verification::VisiblePixelsIdentical => "visible-pixels-identical",
            Verification::ByteIdentical => "byte-identical-copy",
            Verification::CoefficientsIdentical => "dct-coefficients-identical",
            Verification::BitstreamIdentical => "jpeg-bitstream-identical",
            Verification::DecodableOnly => "decodable-lossy-not-compared",
        }
    }
}
