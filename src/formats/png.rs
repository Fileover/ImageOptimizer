//! PNG profile (OxiPNG library + optional Zopfli deflate).
//!
//! Matches `oxipng` upstream defaults: preset 2 (`--opt 2`), libdeflate, `optimize_alpha=false`,
//! `scale_16=false`, metadata preserved unless stripping is requested. Use `--opt max -z` or
//! `--max-compression` for preset 6 + Zopfli.

use super::{Ctx, Encoded, Verification};
use crate::engines::oxipng::{self, PngOptions};
use crate::errors::{Error, Result};
use crate::image::color::{visible_pixels_equal, Raster};
use crate::image::decoder::{decode, DecodeLimits};
use crate::image::encoder::write_png;
use crate::image::identification::{apng_info, png_chunks, Format};
use crate::image::metadata::Metadata;

pub fn options(ctx: &Ctx) -> PngOptions {
    let s = ctx.settings;
    PngOptions {
        preset: s.png_preset,
        zopfli: s.zopfli,
        zopfli_iterations: s.zopfli_iterations,
        zopfli_no_improvement: s.zopfli_no_improvement,
        optimize_alpha: s.optimize_alpha,
        strip_metadata: s.strip_metadata,
        strip_icc: s.strip_icc,
        max_decompressed: Some(((s.max_pixels as usize).saturating_mul(8)).min(usize::MAX / 2)),
    }
}

fn describe(o: &PngOptions) -> String {
    format!(
        "oxipng {}: preset={} deflater={} optimize_alpha={} scale_16=false strip_metadata={} strip_icc={}",
        oxipng::version(),
        o.preset,
        if o.zopfli {
            format!(
                "zopfli(iterations={}, no_improvement={})",
                o.zopfli_iterations,
                o.zopfli_no_improvement.map(|n| n.to_string()).unwrap_or_else(|| "unlimited".into())
            )
        } else {
            "libdeflate".to_string()
        },
        o.optimize_alpha,
        o.strip_metadata,
        o.strip_icc
    )
}

/// PNG -> PNG optimization (also handles APNG; the animation is preserved or the call fails).
pub fn optimize(ctx: &Ctx, src: &[u8]) -> Result<Encoded> {
    let o = options(ctx);
    let bytes = oxipng::optimize(src, &o)?;
    let mut notes = Vec::new();
    if let Some((frames, _)) = apng_info(src) {
        notes.push(format!("APNG with {frames} frames: animation preserved"));
    }
    Ok(Encoded { bytes, lossless: true, settings: describe(&o), notes })
}

/// Pixel -> PNG conversion: write an initial PNG, then optimize it with OxiPNG.
pub fn encode(ctx: &Ctx, r: &Raster, meta: &Metadata) -> Result<Encoded> {
    let mut m = meta.clone();
    m.apply_strip(ctx.settings.strip_metadata, ctx.settings.strip_icc);
    let mut notes = Vec::new();
    if let Some(x) = &m.xmp {
        if std::str::from_utf8(x).is_err() {
            notes.push("XMP packet is not valid UTF-8 and cannot be stored in a PNG iTXt chunk: dropped".into());
            m.xmp = None;
        }
    }
    let initial = write_png(r, &m)?;
    let o = options(ctx);
    let bytes = oxipng::optimize(&initial, &o)?;
    Ok(Encoded { bytes, lossless: true, settings: describe(&o), notes })
}

fn decode_png(bytes: &[u8], ctx: &Ctx) -> Result<Raster> {
    let lim = DecodeLimits { max_pixels: ctx.settings.max_pixels, threads: ctx.threads };
    Ok(decode(bytes, Format::Png, &lim)?.raster)
}

/// Output must decode to the expected samples (strict RGBA16 unless `--alpha`/`--optimize-alpha` is enabled).
pub fn verify_raster(ctx: &Ctx, expected: &Raster, out: &[u8]) -> Result<Verification> {
    let got = decode_png(out, ctx)?;
    if got.width != expected.width || got.height != expected.height {
        return Err(Error::Verification("decoded PNG dimensions differ from the source".into()));
    }
    let a = got.to_rgba16()?;
    let b = expected.to_rgba16()?;
    if ctx.settings.optimize_alpha {
        if !visible_pixels_equal(&a, &b) {
            return Err(Error::Verification("decoded PNG visible pixels or alpha channel differ from the source".into()));
        }
        Ok(Verification::VisiblePixelsIdentical)
    } else {
        if a != b {
            return Err(Error::Verification("decoded PNG pixels differ from the source pixels".into()));
        }
        Ok(Verification::PixelIdentical)
    }
}

/// Chunks that must survive byte-for-byte (not compressed).
const RAW_CHUNKS: [&[u8; 4]; 5] = [b"pHYs", b"sRGB", b"gAMA", b"cHRM", b"cICP"];

/// Metadata in a comparable form. OxiPNG may recompress iCCP/zTXt/iTXt payloads, so those are
/// compared by their decompressed content, not by their stored bytes.
fn metadata_signature(d: &[u8]) -> Result<Vec<String>> {
    let mut sig = Vec::new();
    let reader = png::Decoder::new(std::io::Cursor::new(d))
        .read_info()
        .map_err(|e| Error::codec("png", e.to_string()))?;
    let info = reader.info();
    if let Some(icc) = &info.icc_profile {
        sig.push(format!("icc:{}", hex_digest(icc)));
    }
    if let Some(x) = &info.exif_metadata {
        sig.push(format!("exif:{}", hex_digest(x)));
    }
    for t in &info.uncompressed_latin1_text {
        sig.push(format!("text:{}={}", t.keyword, t.text));
    }
    for t in &info.compressed_latin1_text {
        sig.push(format!("text:{}={}", t.keyword, t.get_text().unwrap_or_default()));
    }
    for t in &info.utf8_text {
        sig.push(format!("itxt:{}={}", t.keyword, t.get_text().unwrap_or_default()));
    }
    for (tag, body) in png_chunks(d) {
        if RAW_CHUNKS.contains(&&tag) {
            sig.push(format!("{}:{}", String::from_utf8_lossy(&tag), hex_digest(body)));
        }
    }
    sig.sort();
    Ok(sig)
}

/// Not cryptographic: just a compact, exact-enough fingerprint (FNV-1a 64 + length).
fn hex_digest(b: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}/{}", h, b.len())
}

/// PNG -> PNG: identical pixels (or visible pixels with `--optimize-alpha`) and, unless stripping
/// was requested, identical ancillary metadata.
pub fn verify_same(ctx: &Ctx, src: &[u8], out: &[u8]) -> Result<Verification> {
    let a = match decode_png(src, ctx) {
        Ok(r) => r,
        Err(Error::Unsupported(_)) => return Ok(Verification::NotRun), // APNG: validated structurally in the engine
        Err(e) => return Err(e),
    };
    let ver = verify_raster(ctx, &a, out)?;
    let s = ctx.settings;
    if !s.strip_metadata && !s.strip_icc && metadata_signature(src)? != metadata_signature(out)? {
        return Err(Error::Verification("ancillary metadata (ICC/EXIF/text/colour chunks) was not preserved".into()));
    }
    Ok(ver)
}
