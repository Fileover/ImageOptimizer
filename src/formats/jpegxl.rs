//! JPEG XL profiles (libjxl / cjxl).
//!
//! Matches `cjxl` upstream defaults:
//! * JPEG -> JXL: lossless JPEG bitstream transcoding by default (`--lossless_jpeg=1`),
//!   `effort = 7`, `brotli_effort = 9`. Passing `--lossless_jpeg=0`, `--lossy`, `--distance > 0`,
//!   or `--quality < 100` decodes the JPEG and re-encodes pixels.
//! * non-JPEG -> JXL: visually lossless Butteraugli `distance = 1.0`, `effort = 7`,
//!   `brotli_effort = 9`. Passing `--lossless`, `--distance=0`, or `--quality=100` selects
//!   mathematically lossless encoding.
//! * `--effort=11` requires `--allow_expert_options` (or `--max-compression`, which sets
//!   effort 11 lossless / 10 lossy and `brotli_effort = 11`).

use super::{Ctx, Encoded, Verification};
use crate::config::{
    Mode, DEFAULT_JXL_BROTLI_EFFORT, DEFAULT_JXL_DISTANCE, DEFAULT_JXL_EFFORT,
    MAX_JXL_BROTLI_EFFORT, MAX_JXL_EFFORT_LOSSLESS, MAX_JXL_EFFORT_LOSSY,
};
use crate::engines::libjxl::{self, JxlOptions};
use crate::errors::{Error, Result};
use crate::image::color::Raster;
use crate::image::metadata::Metadata;

fn default_effort(ctx: &Ctx, lossless: bool) -> i32 {
    let s = ctx.settings;
    if let Some(e) = s.effort {
        e
    } else if s.max_compression {
        if lossless { MAX_JXL_EFFORT_LOSSLESS } else { MAX_JXL_EFFORT_LOSSY }
    } else {
        DEFAULT_JXL_EFFORT
    }
}

fn default_brotli_effort(ctx: &Ctx) -> i32 {
    let s = ctx.settings;
    s.brotli_effort
        .unwrap_or(if s.max_compression { MAX_JXL_BROTLI_EFFORT } else { DEFAULT_JXL_BROTLI_EFFORT })
}

/// Returns true when a JPEG source should use lossless JPEG bitstream transcoding (`cjxl` default).
pub fn should_transcode_jpeg(ctx: &Ctx) -> bool {
    let s = ctx.settings;
    if s.lossless_jpeg == Some(false) {
        return false;
    }
    if s.lossless_jpeg == Some(true) {
        return true;
    }
    match s.mode {
        Mode::Lossless => true,
        Mode::Lossy => false,
        Mode::Default => {
            if let Some(d) = s.distance {
                d <= 0.0
            } else if let Some(q) = s.quality {
                q >= 100.0
            } else {
                true
            }
        }
    }
}

/// Resolves pixel-encoding options for JPEG XL matching `cjxl`.
pub fn options(ctx: &Ctx) -> Result<JxlOptions> {
    let s = ctx.settings;
    let brotli_effort = default_brotli_effort(ctx);
    match s.mode {
        Mode::Lossless => {
            if let Some(q) = s.quality {
                if q < 100.0 {
                    return Err(Error::Invalid(
                        "--quality < 100 cannot be combined with --lossless for JPEG XL".into(),
                    ));
                }
            }
            Ok(JxlOptions {
                lossless: true,
                distance: 0.0,
                effort: default_effort(ctx, true),
                brotli_effort,
                threads: ctx.threads,
                store_metadata: !s.strip_metadata,
            })
        }
        Mode::Lossy => {
            let distance = match (s.distance, s.quality) {
                (Some(d), _) => d,
                (None, Some(q)) => libjxl::distance_from_quality(q),
                (None, None) => DEFAULT_JXL_DISTANCE,
            };
            if distance <= 0.0 {
                return Err(Error::Invalid(
                    "quality 100 / distance 0 means mathematically lossless in JPEG XL; omit --lossy or use --lossless"
                        .into(),
                ));
            }
            Ok(JxlOptions {
                lossless: false,
                distance,
                effort: default_effort(ctx, false),
                brotli_effort,
                threads: ctx.threads,
                store_metadata: !s.strip_metadata,
            })
        }
        Mode::Default => {
            let distance = match (s.distance, s.quality) {
                (Some(d), _) => d,
                (None, Some(q)) => libjxl::distance_from_quality(q),
                (None, None) => DEFAULT_JXL_DISTANCE,
            };
            let lossless = distance <= 0.0;
            Ok(JxlOptions {
                lossless,
                distance: if lossless { 0.0 } else { distance },
                effort: default_effort(ctx, lossless),
                brotli_effort,
                threads: ctx.threads,
                store_metadata: !s.strip_metadata,
            })
        }
    }
}

fn describe(o: &JxlOptions, what: &str) -> String {
    format!(
        "libjxl {}: {what} lossless={} distance={} effort={} brotli_effort={} threads={}",
        libjxl::encoder_version(),
        o.lossless,
        o.distance,
        o.effort,
        o.brotli_effort,
        o.threads
    )
}

pub fn encode(ctx: &Ctx, r: &Raster, meta: &Metadata) -> Result<Encoded> {
    if r.is_cmyk {
        return Err(Error::Unsupported("CMYK sources cannot be converted to JPEG XL".into()));
    }
    let o = options(ctx)?;
    let mut m = meta.clone();
    m.apply_strip(ctx.settings.strip_metadata, ctx.settings.strip_icc);
    let bytes = libjxl::encode(r, &m, &o)?;
    Ok(Encoded { bytes, lossless: o.lossless, settings: describe(&o, "pixel encode"), notes: vec![] })
}

/// Lossless JPEG recompression (`JxlEncoderAddJPEGFrame`).
pub fn transcode_jpeg(ctx: &Ctx, jpeg: &[u8]) -> Result<Encoded> {
    let s = ctx.settings;
    if s.lossless_jpeg == Some(true) {
        if s.mode == Mode::Lossy
            || s.distance.is_some_and(|d| d > 0.0)
            || (s.distance.is_none() && s.quality.is_some_and(|q| q < 100.0))
        {
            return Err(Error::Invalid(
                "--lossless_jpeg=1 cannot be combined with lossy distance/quality (use --lossless_jpeg=0 to re-encode)"
                    .into(),
            ));
        }
    }
    let o = JxlOptions {
        lossless: true,
        distance: 0.0,
        effort: default_effort(ctx, true),
        brotli_effort: default_brotli_effort(ctx),
        threads: ctx.threads,
        store_metadata: !s.strip_metadata,
    };
    let mut notes = Vec::new();
    if s.strip_metadata || s.strip_icc {
        notes.push(
            "--strip-metadata/--strip-icc are ignored for lossless JPEG transcoding: the stored JPEG must stay reconstructable byte-for-byte"
                .into(),
        );
    }
    let bytes = libjxl::encode_jpeg(jpeg, &o).map_err(|e| match e {
        Error::Codec { message, .. } => Error::Unsupported(format!(
            "this JPEG cannot be transcoded losslessly by libjxl ({message}); pass --lossless_jpeg=0 or --lossy to decode and re-encode it"
        )),
        e => e,
    })?;
    Ok(Encoded {
        bytes,
        lossless: true,
        settings: describe(&o, "JPEG reconstruction (lossless transcode)"),
        notes,
    })
}

pub fn verify_pixels(ctx: &Ctx, expected: &Raster, out: &[u8], was_lossless: bool) -> Result<Verification> {
    // keep_orientation: compare the stored pixels, not the displayed (rotated) ones.
    let dec = libjxl::decode(out, ctx.settings.max_pixels, ctx.threads, true)?;
    if dec.raster.width != expected.width || dec.raster.height != expected.height {
        return Err(Error::Verification("output dimensions differ from the source".into()));
    }
    if !was_lossless {
        return Ok(Verification::DecodableOnly);
    }
    if dec.raster.to_rgba16()? != expected.to_rgba16()? {
        return Err(Error::Verification("decoded JPEG XL samples differ from the source samples".into()));
    }
    Ok(Verification::PixelIdentical)
}

pub fn verify_jpeg_reconstruction(ctx: &Ctx, original_jpeg: &[u8], out: &[u8]) -> Result<Verification> {
    match libjxl::decode_jpeg(out, ctx.threads)? {
        Some(rebuilt) if rebuilt == original_jpeg => Ok(Verification::BitstreamIdentical),
        Some(_) => Err(Error::Verification("reconstructed JPEG differs from the original bytes".into())),
        None => Err(Error::Verification("output has no JPEG reconstruction data".into())),
    }
}
