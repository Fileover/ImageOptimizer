//! Per-file pipeline: read -> identify -> decode -> encode/optimize -> verify -> size check ->
//! atomic write. Every stage is timed. Nothing is ever written to the source file; the output is
//! written to a temporary file next to the destination and renamed only after verification.

use crate::cli::TargetFormat;
use crate::config::{Mode, Settings};
use crate::errors::{Error, Result};
use crate::formats::{self, Ctx, Encoded, Verification};
use crate::image::decoder::decode;
use crate::image::identification::{identify, webp_is_lossless, Format};
use crate::utils::filesystem::{long_path, InputFile};
use crate::utils::temporary_files::TempFile;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Output written (smaller than, or a different format from, the source).
    Written,
    /// Optimization did not reduce the size; the unchanged source bytes were copied to the output.
    KeptOriginal,
    /// Dry run: nothing was written.
    DryRun,
    /// Not processed on purpose (not an image, already exists, no optimizer for the format...).
    Skipped,
    Failed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Written => "written",
            Status::KeptOriginal => "kept-original",
            Status::DryRun => "dry-run",
            Status::Skipped => "skipped",
            Status::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Timings {
    pub load_ms: f64,
    pub decode_ms: f64,
    pub encode_ms: f64,
    pub verify_ms: f64,
    pub write_ms: f64,
    pub total_ms: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileResult {
    pub source: PathBuf,
    pub output: Option<PathBuf>,
    pub source_format: Option<&'static str>,
    pub target_format: Option<&'static str>,
    pub status: Status,
    pub mode: &'static str,
    pub source_bytes: u64,
    /// Size of the file that ended up in the output (0 when nothing was written).
    pub output_bytes: u64,
    /// Size of the codec result before the keep-original decision.
    pub encoded_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub verification: Verification,
    pub codec_settings: String,
    pub notes: Vec<String>,
    pub error: Option<String>,
    pub timings: Timings,
}

impl FileResult {
    fn new(source: &Path, settings: &Settings) -> Self {
        FileResult {
            source: source.to_path_buf(),
            output: None,
            source_format: None,
            target_format: None,
            status: Status::Skipped,
            mode: match settings.mode {
                Mode::Default => "default",
                Mode::Lossless => "lossless",
                Mode::Lossy => "lossy",
            },
            source_bytes: 0,
            output_bytes: 0,
            encoded_bytes: 0,
            width: None,
            height: None,
            verification: Verification::NotRun,
            codec_settings: String::new(),
            notes: Vec::new(),
            error: None,
            timings: Timings::default(),
        }
    }

    pub fn saved_bytes(&self) -> i64 {
        if self.output_bytes == 0 {
            0
        } else {
            self.source_bytes as i64 - self.output_bytes as i64
        }
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

/// Routes the work. Returns the target format for a source format and the run settings, or an
/// explanation of why the file is not processed.
pub fn resolve_target(settings: &Settings, src: Format) -> std::result::Result<Format, String> {
    match settings.format {
        TargetFormat::Jxl => Ok(Format::JpegXl),
        TargetFormat::Webp => Ok(Format::WebP),
        TargetFormat::Jpeg => Ok(Format::Jpeg),
        TargetFormat::Png => Ok(Format::Png),
        TargetFormat::Original => match src {
            Format::Png | Format::Jpeg | Format::WebP | Format::JpegXl => Ok(src),
            other => Err(format!(
                "{} has no native optimizer in this application; choose --format png, webp, jpeg or jxl to convert it",
                other.name()
            )),
        },
    }
}

/// Determines whether a `(src, dst)` route should run losslessly under the given settings,
/// matching the upstream default of each underlying tool (`cjxl`, `cwebp`, `jpegoptim`, `MozJPEG`, `oxipng`).
pub fn effective_lossless(settings: &Settings, src: Format, dst: Format, bytes: &[u8]) -> bool {
    match dst {
        Format::Png => true,
        Format::Jpeg => match src {
            Format::Jpeg => match settings.mode {
                Mode::Lossless => true,
                Mode::Lossy => false,
                Mode::Default => settings.quality.is_none(),
            },
            _ => settings.mode == Mode::Lossless, // refused by guard rail when true
        },
        Format::JpegXl => match settings.mode {
            Mode::Lossless => true,
            Mode::Lossy => false,
            Mode::Default => {
                if src == Format::Jpeg {
                    if let Some(d) = settings.distance {
                        d <= 0.0
                    } else if let Some(q) = settings.quality {
                        q >= 100.0
                    } else {
                        true // cjxl default for JPEG inputs: lossless transcode
                    }
                } else {
                    settings.distance == Some(0.0)
                        || (settings.distance.is_none() && settings.quality == Some(100.0))
                }
            }
        },
        Format::WebP => match settings.mode {
            Mode::Lossless => true,
            Mode::Lossy => false,
            Mode::Default => {
                // In `--format original`, preserve lossless WebP (VP8L) unless lossy/quality was requested.
                settings.format == TargetFormat::Original
                    && src == Format::WebP
                    && settings.quality.is_none()
                    && webp_is_lossless(bytes) == Some(true)
            }
        },
        _ => settings.mode == Mode::Lossless,
    }
}

/// Destination path is decided by the caller (it owns the collision-free planner).
pub fn process_file(
    settings: &Settings,
    input: &InputFile,
    dest_for: &dyn Fn(Format, Format) -> PathBuf,
    threads: i32,
    cancel: &AtomicBool,
) -> FileResult {
    let t_total = Instant::now();
    let mut res = FileResult::new(&input.path, settings);
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_file(settings, input, dest_for, threads, cancel, &mut res)
    }));
    match run {
        Ok(Ok(())) => {}
        Ok(Err(Error::Cancelled)) => {
            res.status = Status::Skipped;
            res.error = Some("cancelled".into());
        }
        Ok(Err(e)) => {
            res.status = match e {
                // Deliberate refusals are reported as skipped, real failures as failed.
                Error::Unsupported(_) | Error::LossyRequired(_) => Status::Skipped,
                _ => Status::Failed,
            };
            res.error = Some(e.to_string());
            res.output = None;
            res.output_bytes = 0;
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            res.status = Status::Failed;
            res.error = Some(format!("internal error (panic): {msg}"));
        }
    }
    res.timings.total_ms = ms(t_total);
    res
}

fn run_file(
    settings: &Settings,
    input: &InputFile,
    dest_for: &dyn Fn(Format, Format) -> PathBuf,
    threads: i32,
    cancel: &AtomicBool,
    res: &mut FileResult,
) -> Result<()> {
    let ctx = Ctx { settings, threads };

    // ---- load
    let t = Instant::now();
    let bytes = std::fs::read(long_path(&input.path)).map_err(|e| Error::io(&input.path, e))?;
    res.timings.load_ms = ms(t);
    res.source_bytes = bytes.len() as u64;
    check_cancel(cancel)?;

    let Some(src) = identify(&bytes) else {
        res.notes.push("not a supported image (detected by file signature)".into());
        return Err(Error::Unsupported("not a supported image (no known signature)".into()));
    };
    res.source_format = Some(src.name());
    let dst = resolve_target(settings, src).map_err(Error::Unsupported)?;
    res.target_format = Some(dst.name());
    let planned_lossless = effective_lossless(settings, src, dst, &bytes);
    res.mode = if planned_lossless { "lossless" } else { "lossy" };

    let dest = dest_for(src, dst);
    res.output = Some(dest.clone());
    let dest_lp = long_path(&dest);

    // Defensive: never replace the source itself.
    if same_file(&dest_lp, &input.path) {
        return Err(Error::Invalid("output path equals the source path; refusing to overwrite the source".into()));
    }

    if dest.exists() && !settings.overwrite {
        res.output = Some(dest.clone());
        return Err(Error::Unsupported(format!(
            "output already exists (use --overwrite): {}",
            dest.display()
        )));
    }

    if settings.dry_run {
        res.status = Status::DryRun;
        res.codec_settings = format!("would convert {} -> {} ({})", src.name(), dst.name(), res.mode);
        return Ok(());
    }

    // ---- encode
    let mut decoded_raster = None;
    let mut meta = None;
    let encoded = encode_route(&ctx, src, dst, &bytes, planned_lossless, cancel, res, &mut decoded_raster, &mut meta)?;
    check_cancel(cancel)?;

    res.mode = if encoded.lossless { "lossless" } else { "lossy" };
    res.encoded_bytes = encoded.bytes.len() as u64;
    res.codec_settings = encoded.settings.clone();
    res.notes.extend(encoded.notes.iter().cloned());

    // ---- verify (before anything is written)
    if settings.verify {
        let t = Instant::now();
        res.verification =
            verify_route(&ctx, src, dst, &bytes, &encoded.bytes, decoded_raster.as_ref(), encoded.lossless)?;
        res.timings.verify_ms = ms(t);
    } else {
        res.notes.push("verification disabled (--verify=false)".into());
    }
    check_cancel(cancel)?;

    // ---- keep-original decision (only for same-format optimization)
    let mut payload: &[u8] = &encoded.bytes;
    res.status = Status::Written;
    if src == dst && encoded.bytes.len() >= bytes.len() && !settings.keep_larger {
        payload = &bytes;
        res.status = Status::KeptOriginal;
        res.notes.push(format!(
            "optimized result ({} bytes) is not smaller than the source ({} bytes): source bytes copied unchanged (use --keep-larger to write the optimized file anyway)",
            encoded.bytes.len(),
            bytes.len()
        ));
        res.verification = Verification::ByteIdentical;
    } else if src != dst && encoded.bytes.len() >= bytes.len() {
        res.notes.push("converted file is larger than the source (format conversion was requested, so it was written)".into());
    }

    // ---- write
    let t = Instant::now();
    if let Some(dir) = dest.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(long_path(dir)).map_err(|e| Error::io(dir, e))?;
    }
    let tmp = TempFile::write_next_to(&dest_lp, payload)?;
    tmp.persist(&dest_lp, settings.overwrite)?;
    res.output_bytes = payload.len() as u64;
    res.timings.write_ms = ms(t);
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_route(
    ctx: &Ctx,
    src: Format,
    dst: Format,
    bytes: &[u8],
    lossless: bool,
    cancel: &AtomicBool,
    res: &mut FileResult,
    raster_out: &mut Option<crate::image::color::Raster>,
    meta_out: &mut Option<crate::image::metadata::Metadata>,
) -> Result<Encoded> {
    // Routes that work on the compressed bitstream and never decode pixels.
    match (src, dst) {
        (Format::Png, Format::Png) => {
            let t = Instant::now();
            let e = formats::png::optimize(ctx, bytes)?;
            res.timings.encode_ms = ms(t);
            return Ok(e);
        }
        (Format::Jpeg, Format::Jpeg) => {
            let t = Instant::now();
            let e = formats::jpeg::optimize(ctx, bytes)?;
            res.timings.encode_ms = ms(t);
            return Ok(e);
        }
        (Format::Jpeg, Format::JpegXl) if formats::jpegxl::should_transcode_jpeg(ctx) => {
            let t = Instant::now();
            let e = formats::jpegxl::transcode_jpeg(ctx, bytes)?;
            res.timings.encode_ms = ms(t);
            return Ok(e);
        }
        _ => {}
    }

    // Strict lossless guard rails before spending time on decoding.
    if dst == Format::Jpeg && ctx.strict_lossless() {
        return Err(Error::LossyRequired(format!(
            "{} -> JPEG requires lossy encoding (JPEG is a lossy codec); omit --lossless to encode with MozJPEG",
            src.name()
        )));
    }
    if src == Format::WebP && dst == Format::WebP && lossless && webp_is_lossless(bytes) == Some(false) {
        return Err(Error::LossyRequired(
            "source is a lossy WebP: it cannot be optimized losslessly (re-encoding losslessly would only grow it); omit --lossless to re-encode it"
                .into(),
        ));
    }

    // ---- decode
    let t = Instant::now();
    let dec = decode(bytes, src, &ctx.limits())?;
    res.timings.decode_ms = ms(t);
    res.width = Some(dec.raster.width);
    res.height = Some(dec.raster.height);
    check_cancel(cancel)?;

    let mut notes = Vec::new();
    let src_is_lossy = src == Format::Jpeg || (src == Format::WebP && webp_is_lossless(bytes) == Some(false));
    if src_is_lossy && lossless && dst != src {
        notes.push(format!(
            "source {} is lossy: output preserves its decoded pixels exactly, but does not restore quality that was already lost",
            src.name()
        ));
    }
    if !lossless && src_is_lossy {
        notes.push("lossy re-encode of an already lossy source: generation loss applies".into());
    }

    let t = Instant::now();
    let mut enc = match dst {
        Format::JpegXl => formats::jpegxl::encode(ctx, &dec.raster, &dec.metadata)?,
        Format::WebP => formats::webp::encode_with_mode(ctx, &dec.raster, &dec.metadata, lossless)?,
        Format::Jpeg => formats::jpeg::encode(ctx, &dec.raster, &dec.metadata)?,
        Format::Png => formats::png::encode(ctx, &dec.raster, &dec.metadata)?,
        other => return Err(Error::Unsupported(format!("{} is not an output format", other.name()))),
    };
    res.timings.encode_ms = ms(t);
    enc.notes.extend(notes);
    *raster_out = Some(dec.raster);
    *meta_out = Some(dec.metadata);
    Ok(enc)
}

fn verify_route(
    ctx: &Ctx,
    src: Format,
    dst: Format,
    src_bytes: &[u8],
    out: &[u8],
    raster: Option<&crate::image::color::Raster>,
    was_lossless: bool,
) -> Result<Verification> {
    match (src, dst) {
        (Format::Png, Format::Png) => formats::png::verify_same(ctx, src_bytes, out),
        (Format::Jpeg, Format::Jpeg) => formats::jpeg::verify_optimized(ctx, src_bytes, out, was_lossless),
        (Format::Jpeg, Format::JpegXl) if formats::jpegxl::should_transcode_jpeg(ctx) => {
            formats::jpegxl::verify_jpeg_reconstruction(ctx, src_bytes, out)
        }
        (_, Format::JpegXl) => formats::jpegxl::verify_pixels(ctx, need(raster)?, out, was_lossless),
        (_, Format::WebP) => formats::webp::verify(ctx, need(raster)?, out, was_lossless),
        (_, Format::Png) => formats::png::verify_raster(ctx, need(raster)?, out),
        (_, Format::Jpeg) => formats::jpeg::verify_decodable(ctx, raster, out),
        _ => Ok(Verification::NotRun),
    }
}

fn need<T>(o: Option<T>) -> Result<T> {
    o.ok_or_else(|| Error::Invalid("internal error: decoded raster missing for verification".into()))
}
