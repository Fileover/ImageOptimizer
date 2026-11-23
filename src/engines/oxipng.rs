//! OxiPNG engine (library API) with Zopfli deflate.

use crate::errors::{Error, Result};
use crate::image::identification::apng_info;
use oxipng::{indexset, Deflater, Options, StripChunks, ZopfliOptions};
use std::num::NonZeroU64;

#[derive(Clone, Debug)]
pub struct PngOptions {
    /// Preset 0..=6 (`oxipng -o`); 6 is the highest (`Options::max_compression`).
    pub preset: u8,
    pub zopfli: bool,
    /// Zopfli `iteration_count` (`--zi`). The API has no upper bound.
    pub zopfli_iterations: u64,
    /// Zopfli `iterations_without_improvement` (`--ziwi`); `None` = unlimited.
    pub zopfli_no_improvement: Option<u64>,
    /// Optimize RGB values of fully transparent pixels (`oxipng --alpha`).
    pub optimize_alpha: bool,
    pub strip_metadata: bool,
    pub strip_icc: bool,
    /// Upper bound for inflated image data (protects against decompression bombs).
    pub max_decompressed: Option<usize>,
}

pub fn version() -> String {
    // Kept in sync with Cargo.toml / native/dependencies.json.
    "10.2.1".to_string()
}

pub fn to_oxipng_options(o: &PngOptions) -> Options {
    let mut opts = Options::from_preset(o.preset);
    // Preserve exact RGB under alpha=0 unless --optimize-alpha was requested, and never scale 16-bit samples.
    opts.optimize_alpha = o.optimize_alpha;
    opts.scale_16 = false;
    opts.force = false;
    opts.fix_errors = false;
    opts.max_decompressed_size = o.max_decompressed;
    if o.zopfli {
        opts.deflater = Deflater::Zopfli(ZopfliOptions {
            iteration_count: NonZeroU64::new(o.zopfli_iterations.max(1)).unwrap(),
            iterations_without_improvement: o
                .zopfli_no_improvement
                .and_then(NonZeroU64::new)
                .unwrap_or(NonZeroU64::MAX),
            ..Default::default()
        });
    }
    opts.strip = match (o.strip_metadata, o.strip_icc) {
        (false, false) => StripChunks::None,
        (true, false) => StripChunks::Safe,
        (false, true) => StripChunks::Strip(indexset! {*b"iCCP"}),
        (true, true) => StripChunks::Keep(indexset! {*b"cICP", *b"sRGB", *b"pHYs", *b"acTL", *b"fcTL", *b"fdAT"}),
    };
    opts
}

/// Optimizes a PNG in memory. APNG input is preserved: if the optimized output lost any
/// animation information the call fails instead of returning a static image.
pub fn optimize(data: &[u8], o: &PngOptions) -> Result<Vec<u8>> {
    let opts = to_oxipng_options(o);
    let out = oxipng::optimize_from_memory(data, &opts).map_err(|e| Error::codec("oxipng", e.to_string()))?;
    if let Some(before) = apng_info(data) {
        match apng_info(&out) {
            Some(after) if after == before => {}
            _ => {
                return Err(Error::Verification(
                    "APNG animation information was not preserved by oxipng; original kept".into(),
                ))
            }
        }
    }
    Ok(out)
}
