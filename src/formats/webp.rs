//! WebP profiles (libwebp / cwebp).
//!
//! Follows `cwebp` upstream defaults, tuned for maximum compression effort:
//! * Default (lossy): quality 75 (`cwebp -q 75`), method 6 (`cwebp -m 6`), alpha quality 100
//!   (`cwebp -alpha_q 100`), optional `--sharp-yuv`.
//! * Lossless (`--lossless`, or lossless WebP in `--format original`): `lossless=1`, `exact=1`
//!   (hidden RGB under transparent pixels is preserved), method 6, quality 100 (`cwebp -z 9`).

use super::{Ctx, Encoded, Verification};
use crate::config::{DEFAULT_WEBP_METHOD, DEFAULT_WEBP_QUALITY};
use crate::engines::libwebp::{self, WebpOptions};
use crate::errors::{Error, Result};
use crate::image::color::Raster;
use crate::image::metadata::Metadata;

pub fn options_for(ctx: &Ctx, lossless: bool) -> WebpOptions {
    let s = ctx.settings;
    if lossless {
        WebpOptions {
            lossless: true,
            quality: 100.0,
            method: s.method.unwrap_or(DEFAULT_WEBP_METHOD),
            alpha_quality: 100,
            threads: ctx.threads,
            sharp_yuv: false,
        }
    } else {
        WebpOptions {
            lossless: false,
            quality: s.quality.unwrap_or(DEFAULT_WEBP_QUALITY),
            method: s.method.unwrap_or(DEFAULT_WEBP_METHOD),
            alpha_quality: s.alpha_quality.unwrap_or(100),
            threads: ctx.threads,
            sharp_yuv: s.sharp_yuv,
        }
    }
}

pub fn options(ctx: &Ctx) -> WebpOptions {
    options_for(ctx, ctx.strict_lossless())
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
    let mut notes = Vec::new();
    let r8 = prepare(lossless, r, &mut notes)?;
    let mut m = meta.clone();
    m.apply_strip(ctx.settings.strip_metadata, ctx.settings.strip_icc);
    let o = options_for(ctx, lossless);
    let bytes = libwebp::encode(&r8, &m, &o)?;
    let settings = format!(
        "libwebp {}: lossless={} quality={} method={} alpha_quality={} exact={} sharp_yuv={} threads={}",
        libwebp::version(),
        o.lossless,
        o.quality,
        o.method,
        o.alpha_quality,
        o.lossless,
        !o.lossless && o.sharp_yuv,
        o.threads
    );
    Ok(Encoded { bytes, lossless: o.lossless, settings, notes })
}

pub fn encode(ctx: &Ctx, r: &Raster, meta: &Metadata) -> Result<Encoded> {
    encode_with_mode(ctx, r, meta, ctx.strict_lossless())
}

/// `expected` is the raster that was *intended* to be stored (before 8-bit/gray preparation
/// would have changed it - in lossless mode preparation is exact).
pub fn verify(ctx: &Ctx, expected: &Raster, out: &[u8], was_lossless: bool) -> Result<Verification> {
    let dec = libwebp::decode(out, ctx.settings.max_pixels)?;
    if dec.raster.width != expected.width || dec.raster.height != expected.height {
        return Err(Error::Verification("output dimensions differ from the source".into()));
    }
    if !was_lossless {
        return Ok(Verification::DecodableOnly);
    }
    if dec.raster.to_rgba16()? != expected.to_rgba16()? {
        return Err(Error::Verification("decoded WebP pixels differ from the source pixels".into()));
    }
    Ok(Verification::PixelIdentical)
}
