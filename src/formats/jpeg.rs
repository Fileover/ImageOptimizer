//! JPEG profiles (jpegoptim core + MozJPEG).
//!
//! Follows upstream tool defaults, tuned for maximum compression:
//! * JPEG -> JPEG (`jpegoptim` core on MozJPEG): lossless Huffman optimization by default
//!   (coefficients and quantization tables untouched). Passing `--quality N` (or `--lossy`)
//!   enables `jpegoptim --max` style lossy recompression, automatically keeping the lossless
//!   result whenever it is already smaller than or equal to the lossy re-encode.
//! * non-JPEG -> JPEG (MozJPEG `cjpeg` profile): lossy by default (quality 75, progressive,
//!   trellis quantization and optimized scans). Refused only when `--lossless` is passed.

use super::{Ctx, Encoded, Verification};
use crate::cli::JpegModeArg;
use crate::config::{Mode, Settings, DEFAULT_JPEG_QUALITY};
use crate::engines::jpegoptim::{self, JpegReport, JpegoptimOptions, ProgressiveMode};
use crate::engines::mozjpeg;
use crate::errors::{Error, Result};
use crate::image::color::Raster;
use crate::image::metadata::Metadata;

fn quality(ctx: &Ctx) -> u8 {
    ctx.settings.quality.unwrap_or(DEFAULT_JPEG_QUALITY).round().clamp(0.0, 100.0) as u8
}

/// Target lossy quality for JPEG -> JPEG optimization (`None` = lossless).
pub fn optimize_target_quality(ctx: &Ctx) -> Option<u8> {
    match ctx.settings.mode {
        Mode::Lossless => None,
        Mode::Lossy => Some(quality(ctx)),
        Mode::Default => ctx.settings.quality.map(|q| q.round().clamp(0.0, 100.0) as u8),
    }
}

fn run_jpegoptim(src: &[u8], q: Option<u8>, s: &Settings) -> Result<(Vec<u8>, JpegReport, &'static str)> {
    let make_opts = |progressive: ProgressiveMode| JpegoptimOptions {
        quality: q,
        progressive,
        strip_com: s.strip_metadata,
        strip_exif: s.strip_metadata,
        strip_iptc: s.strip_metadata,
        strip_icc: s.strip_icc,
        strip_xmp: s.strip_metadata,
        max_pixels: s.max_pixels,
    };

    match s.jpeg_mode {
        JpegModeArg::Keep => {
            let (b, r) = jpegoptim::optimize(src, &make_opts(ProgressiveMode::Keep))?;
            Ok((b, r, "keep-input"))
        }
        JpegModeArg::Progressive => {
            let (b, r) = jpegoptim::optimize(src, &make_opts(ProgressiveMode::Progressive))?;
            Ok((b, r, "progressive"))
        }
        JpegModeArg::Baseline => {
            let (b, r) = jpegoptim::optimize(src, &make_opts(ProgressiveMode::Baseline))?;
            Ok((b, r, "baseline"))
        }
        JpegModeArg::Auto => {
            let (b_base, r_base) = jpegoptim::optimize(src, &make_opts(ProgressiveMode::Baseline))?;
            let (b_prog, r_prog) = jpegoptim::optimize(src, &make_opts(ProgressiveMode::Progressive))?;
            if b_prog.len() <= b_base.len() {
                Ok((b_prog, r_prog, "auto(progressive)"))
            } else {
                Ok((b_base, r_base, "auto(baseline)"))
            }
        }
    }
}

pub fn optimize(ctx: &Ctx, src: &[u8]) -> Result<Encoded> {
    let s = ctx.settings;
    if ctx.strict_lossless() && s.quality.is_some() {
        return Err(Error::Invalid("--quality cannot be combined with --lossless for JPEG".into()));
    }
    let target_q = optimize_target_quality(ctx);
    let mut notes = Vec::new();

    let (bytes, rep, prog_label, used_q) = if let Some(q) = target_q {
        // Upstream jpegoptim -m<q> behavior: run lossless optimization too and only use the lossy
        // recompression if it actually produces a smaller file (avoids pointless generation loss).
        let (ll_bytes, ll_rep, ll_prog) = run_jpegoptim(src, None, s)?;
        if ll_rep.trailing_nonzero {
            return Err(Error::Unsupported(format!(
                "{} bytes of data follow the JPEG end-of-image marker (multi-picture/appended data); rewriting would invalidate it, original kept",
                ll_rep.trailing_bytes
            )));
        }
        let (ly_bytes, ly_rep, ly_prog) = run_jpegoptim(src, Some(q), s)?;
        if ll_bytes.len() <= ly_bytes.len() {
            notes.push(format!(
                "lossless Huffman optimization ({} bytes) was smaller than or equal to lossy recompression at quality {} ({} bytes); kept lossless result (jpegoptim --max behavior)",
                ll_bytes.len(),
                q,
                ly_bytes.len()
            ));
            (ll_bytes, ll_rep, ll_prog, None)
        } else {
            (ly_bytes, ly_rep, ly_prog, Some(q))
        }
    } else {
        let (b, r, p) = run_jpegoptim(src, None, s)?;
        if r.trailing_nonzero {
            return Err(Error::Unsupported(format!(
                "{} bytes of data follow the JPEG end-of-image marker (multi-picture/appended data); rewriting would invalidate it, original kept",
                r.trailing_bytes
            )));
        }
        (b, r, p, None)
    };

    if rep.arithmetic_in {
        notes.push("arithmetic-coded input was rewritten with Huffman coding (lossless, wider compatibility)".into());
    }
    if s.strip_metadata || s.strip_icc {
        notes.push(
            "metadata stripping uses jpegoptim's marker policy (JFXX/Adobe markers are dropped unless libjpeg re-creates them)"
                .into(),
        );
    }
    let lossless = used_q.is_none();
    let settings = format!(
        "jpegoptim 1.5.6 core on {}: mode={} progressive={} quality={} strip_metadata={} strip_icc={} markers {}->{}",
        mozjpeg::version(),
        if lossless { "lossless" } else { "lossy" },
        prog_label,
        used_q.map(|q| q.to_string()).unwrap_or_else(|| "-".into()),
        s.strip_metadata,
        s.strip_icc,
        rep.markers_in,
        rep.markers_written
    );
    Ok(Encoded { bytes, lossless, settings, notes })
}

/// Pixel -> JPEG conversion (MozJPEG). Refused only in `--lossless` mode (enforced by pipeline).
pub fn encode(ctx: &Ctx, r: &Raster, meta: &Metadata) -> Result<Encoded> {
    if r.is_cmyk {
        return Err(Error::Unsupported("CMYK sources cannot be converted to JPEG".into()));
    }
    let mut notes = Vec::new();
    let mut r = r.clone();
    if r.depth() == 16 {
        notes.push("16-bit samples reduced to 8-bit for JPEG".into());
        r = r.to_8bit();
    }
    if r.has_alpha() {
        if r.has_transparency() {
            notes.push("transparency cannot be stored in JPEG: image was composited on a white background".into());
        } else {
            notes.push("fully opaque alpha channel removed for JPEG".into());
        }
        r = r.flatten_on_white()?;
    }
    let mut m = meta.clone();
    m.apply_strip(ctx.settings.strip_metadata, ctx.settings.strip_icc);
    if m.exif.as_ref().map(|e| e.len() + 6 > 65533).unwrap_or(false) {
        notes.push("EXIF block too large for a JPEG APP1 segment: dropped".into());
        m.exif = None;
    }
    if m.xmp.as_ref().map(|x| x.len() + 29 > 65533).unwrap_or(false) {
        notes.push("XMP packet too large for a JPEG APP1 segment (extended XMP is not written): dropped".into());
        m.xmp = None;
    }
    let q = quality(ctx);
    let prog = ctx.settings.jpeg_mode != JpegModeArg::Baseline;
    let bytes = mozjpeg::encode(&r, &m, q, prog)?;
    let settings = format!(
        "{}: quality={q} progressive={prog} trellis/optimized scans (MozJPEG max-compression profile)",
        mozjpeg::version()
    );
    Ok(Encoded { bytes, lossless: false, settings, notes })
}

pub fn verify_optimized(ctx: &Ctx, src: &[u8], out: &[u8], was_lossless: bool) -> Result<Verification> {
    if was_lossless {
        let s = ctx.settings;
        let markers_must_match = !(s.strip_metadata || s.strip_icc);
        if !jpegoptim::coefficients_equal(src, out, markers_must_match)? {
            return Err(Error::Verification(
                "DCT coefficients, quantization tables or markers differ after lossless optimization".into(),
            ));
        }
        Ok(Verification::CoefficientsIdentical)
    } else {
        verify_decodable(ctx, None, out)
    }
}

pub fn verify_decodable(ctx: &Ctx, expected: Option<&Raster>, out: &[u8]) -> Result<Verification> {
    let d = mozjpeg::decode(out, ctx.settings.max_pixels)?;
    if let Some(e) = expected {
        if d.raster.width != e.width || d.raster.height != e.height {
            return Err(Error::Verification("output dimensions differ from the source".into()));
        }
    }
    Ok(Verification::DecodableOnly)
}
