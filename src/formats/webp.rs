//! WebP profiles (libwebp / cwebp).
//!
//! Matches `cwebp` upstream defaults:
//! * Default (lossy): `quality = 75` (`-q 75`), `method = 4` (`-m 4`), `alpha_q = 100`,
//!   `sharp_yuv = false`.
//! * Lossless (`--lossless`, `--lossless-preset 0..9`, or lossless WebP in `--format original`):
//!   `lossless = 1`, `quality = 75` (effort trade-off, `100` under `--max-compression`),
//!   `method = 4` (`6` under `--max-compression`), `exact = false` unless `--exact` is passed.

use super::{Ctx, Encoded, Verification};
use crate::config::{DEFAULT_WEBP_METHOD, DEFAULT_WEBP_QUALITY, MAX_WEBP_METHOD};
use crate::engines::libwebp::{self, WebpOptions};
use crate::errors::{Error, Result};
use crate::image::color::{visible_pixels_equal, Raster};
use crate::image::metadata::Metadata;

pub fn options_for(ctx: &Ctx, lossless: bool) -> WebpOptions {
    let s = ctx.settings;
    let is_lossless = lossless || s.lossless_preset.is_some();
    if is_lossless {
        if let Some(preset) = s.lossless_preset {
            WebpOptions {
                lossless: true,
                lossless_preset: Some(preset),
                quality: s.quality,
                method: s.method,
                alpha_quality: 100,
                threads: ctx.threads,
                exact: s.exact,
                sharp_yuv: false,
            }
        } else {
            let def_q = if s.max_compression { 100.0 } else { DEFAULT_WEBP_QUALITY };
            let def_m = if s.max_compression { MAX_WEBP_METHOD } else { DEFAULT_WEBP_METHOD };
            WebpOptions {
                lossless: true,
                lossless_preset: None,
                quality: Some(s.quality.unwrap_or(def_q)),
                method: Some(s.method.unwrap_or(def_m)),
                alpha_quality: 100,
                threads: ctx.threads,
                exact: s.exact,
                sharp_yuv: false,
            }
        }
    } else {
        let def_m = if s.max_compression { MAX_WEBP_METHOD } else { DEFAULT_WEBP_METHOD };
        WebpOptions {
            lossless: false,
            lossless_preset: None,
            quality: Some(s.quality.unwrap_or(DEFAULT_WEBP_QUALITY)),
            method: Some(s.method.unwrap_or(def_m)),
            alpha_quality: s.alpha_quality.unwrap_or(100),
            threads: ctx.threads,
            exact: false,
            sharp_yuv: s.sharp_yuv,
        }
    }
}

pub fn options(ctx: &Ctx) -> WebpOptions {
    options_for(ctx, ctx.strict_lossless() || ctx.settings.lossless_preset.is_some())
}

/// Brings a raster into the 8-bit RGB/RGBA layout libwebp accepts.
fn prepare(lossless: bool, r: &Raster, notes: &mut Vec<String>) -> Result<Raster> {
    if r.is_cmyk {
        return Err(Error::Unsupported("CMYK sources cannot be converted to WebP".into()));
    }
    let mut r = r.clone();
    if r.depth() == 16 {
        if lossless {
            return Err(Error::LossyRequired(
                "WebP stores 8 bits per channel: a 16-bit source cannot be converted losslessly (omit --lossless, or use --format png/jxl)"
                    .into(),
            ));
        }
        notes.push("16-bit samples reduced to 8-bit for WebP".into());
        r = r.to_8bit();
    }
    if r.channels <= 2 {
        r = r.expand_gray()?; // exact: gray -> RGB with identical channels
    }
    Ok(r)
}

pub fn encode_with_mode(ctx: &Ctx, r: &Raster, meta: &Metadata, lossless: bool) -> Result<Encoded> {
    let effective_lossless = lossless || ctx.settings.lossless_preset.is_some();
    let mut notes = Vec::new();
    let r8 = prepare(effective_lossless, r, &mut notes)?;
    let mut m = meta.clone();
    m.apply_strip(ctx.settings.strip_metadata, ctx.settings.strip_icc);
    let o = options_for(ctx, effective_lossless);
    let bytes = libwebp::encode(&r8, &m, &o)?;
    let settings = format!(
        "libwebp {}: lossless={} preset={} quality={} method={} alpha_q={} exact={} sharp_yuv={} threads={}",
        libwebp::version(),
        o.lossless,
        o.lossless_preset.map(|z| z.to_string()).unwrap_or_else(|| "-".into()),
        o.quality.map(|q| q.to_string()).unwrap_or_else(|| "preset".into()),
        o.method.map(|m| m.to_string()).unwrap_or_else(|| "preset".into()),
        o.alpha_quality,
        o.lossless && o.exact,
        !o.lossless && o.sharp_yuv,
        o.threads
    );
    Ok(Encoded { bytes, lossless: o.lossless, settings, notes })
}

pub fn encode(ctx: &Ctx, r: &Raster, meta: &Metadata) -> Result<Encoded> {
    encode_with_mode(ctx, r, meta, ctx.strict_lossless())
}

/// `expected` is the raster that was *intended* to be stored.
pub fn verify(ctx: &Ctx, expected: &Raster, out: &[u8], was_lossless: bool) -> Result<Verification> {
    let dec = libwebp::decode(out, ctx.settings.max_pixels)?;
    if dec.raster.width != expected.width || dec.raster.height != expected.height {
        return Err(Error::Verification("output dimensions differ from the source".into()));
    }
    if !was_lossless {
        return Ok(Verification::DecodableOnly);
    }
    let a = dec.raster.to_rgba16()?;
    let b = expected.to_rgba16()?;
    if ctx.settings.exact {
        if a != b {
            return Err(Error::Verification("decoded WebP pixels differ from the source pixels".into()));
        }
        Ok(Verification::PixelIdentical)
    } else if a == b {
        Ok(Verification::PixelIdentical)
    } else if visible_pixels_equal(&a, &b) {
        Ok(Verification::VisiblePixelsIdentical)
    } else {
        Err(Error::Verification("decoded WebP visible pixels or alpha channel differ from the source".into()))
    }
}
