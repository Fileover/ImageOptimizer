//! Resolved run settings: CLI values > JSON configuration file > built-in defaults.

use crate::cli::{Cli, JpegModeArg, TargetFormat};
use crate::errors::{Error, Result};
use crate::image::resize::{ResizeFilter, ResizeSpec};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Upstream tool defaults per codec, tuned for maximum compression effort.
    Default,
    /// Strict mathematically lossless mode across every codec.
    Lossless,
    /// Force lossy compression even where the upstream default is lossless.
    Lossy,
}

/// Optional JSON configuration (every key mirrors a command line option).
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub format: Option<TargetFormat>,
    pub lossless: Option<bool>,
    pub lossy: Option<bool>,
    pub quality: Option<f32>,
    pub distance: Option<f32>,
    pub effort: Option<i32>,
    pub brotli_effort: Option<i32>,
    pub method: Option<i32>,
    pub alpha_quality: Option<i32>,
    pub sharp_yuv: Option<bool>,
    pub jpeg_mode: Option<JpegModeArg>,
    pub png_preset: Option<u8>,
    pub optimize_alpha: Option<bool>,
    pub zopfli: Option<bool>,
    pub zopfli_iterations: Option<u64>,
    pub zopfli_no_improvement: Option<u64>,
    pub resize: Option<String>,
    pub resize_filter: Option<ResizeFilter>,
    pub strip_metadata: Option<bool>,
    pub strip_icc: Option<bool>,
    pub jobs: Option<usize>,
    pub threads: Option<usize>,
    pub overwrite: Option<bool>,
    pub recursive: Option<bool>,
    pub keep_larger: Option<bool>,
    pub report: Option<bool>,
    pub verify: Option<bool>,
    pub max_pixels: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Settings {
    pub input: PathBuf,
    pub output: PathBuf,
    pub format: TargetFormat,
    /// True when `--format` (or `format` in JSON config) was explicitly set by the user.
    #[serde(skip)]
    pub format_explicit: bool,
    pub mode: Mode,
    /// Quality as given by the user; upstream codec defaults apply when omitted.
    pub quality: Option<f32>,
    pub distance: Option<f32>,
    pub effort: Option<i32>,
    pub brotli_effort: Option<i32>,
    pub method: Option<i32>,
    pub alpha_quality: Option<i32>,
    pub sharp_yuv: bool,
    pub jpeg_mode: JpegModeArg,
    pub png_preset: u8,
    pub optimize_alpha: bool,
    pub zopfli: bool,
    pub zopfli_iterations: u64,
    pub zopfli_no_improvement: Option<u64>,
    pub resize: Option<ResizeSpec>,
    pub resize_filter: ResizeFilter,
    pub strip_metadata: bool,
    pub strip_icc: bool,
    pub jobs: Option<usize>,
    pub threads: Option<usize>,
    pub overwrite: bool,
    pub dry_run: bool,
    pub recursive: bool,
    pub keep_larger: bool,
    pub report: bool,
    pub verify: bool,
    pub max_pixels: u64,
    pub quiet: bool,
    pub verbose: u8,
}

pub const DEFAULT_MAX_PIXELS: u64 = 268_435_456;
/// OxiPNG's API has no maximum for Zopfli iterations; this is a documented practical default.
pub const DEFAULT_ZOPFLI_ITERATIONS: u64 = 60;
pub const DEFAULT_ZOPFLI_NO_IMPROVEMENT: u64 = 15;

/// Upstream codec defaults (paired with maximum compression effort).
pub const DEFAULT_WEBP_QUALITY: f32 = 75.0; // cwebp default -q 75
pub const DEFAULT_WEBP_METHOD: i32 = 6; // cwebp maximum -m 6
pub const DEFAULT_JPEG_QUALITY: f32 = 75.0; // MozJPEG cjpeg default -quality 75
pub const DEFAULT_JXL_DISTANCE: f32 = 1.0; // cjxl default -d 1.0 (visually lossless)
pub const DEFAULT_JXL_EFFORT_LOSSY: i32 = 9; // highest standard lossy effort
pub const DEFAULT_JXL_EFFORT_LOSSLESS: i32 = 11; // maximum lossless effort (enables libjxl expert options)
pub const DEFAULT_JXL_BROTLI_EFFORT: i32 = 11; // maximum Brotli effort for metadata boxes & JPEG transcoding

fn range<T: PartialOrd + std::fmt::Display>(name: &str, v: T, lo: T, hi: T) -> Result<()> {
    if v < lo || v > hi {
        return Err(Error::Invalid(format!("{name} must be between {lo} and {hi} (got {v})")));
    }
    Ok(())
}

impl Settings {
    pub fn resolve(cli: &Cli) -> Result<Settings> {
        let file: FileConfig = match &cli.config {
            Some(p) => {
                let text = std::fs::read_to_string(p).map_err(|e| Error::io(p, e))?;
                serde_json::from_str(&text)
                    .map_err(|e| Error::Invalid(format!("invalid configuration file {}: {e}", p.display())))?
            }
            None => FileConfig::default(),
        };
        let input = cli
            .input
            .clone()
            .or_else(|| cli.input_pos.clone())
            .or(file.input)
            .ok_or_else(|| Error::Invalid("--input (or INPUT path) is required".into()))?;
        let output = cli
            .output
            .clone()
            .or_else(|| cli.output_pos.clone())
            .or(file.output)
            .ok_or_else(|| Error::Invalid("--output (or OUTPUT path) is required".into()))?;

        let mode = if cli.lossless {
            Mode::Lossless
        } else if cli.lossy {
            Mode::Lossy
        } else {
            match (file.lossless.unwrap_or(false), file.lossy.unwrap_or(false)) {
                (true, true) => {
                    return Err(Error::Invalid(
                        "configuration file cannot set both 'lossless' and 'lossy' to true".into(),
                    ))
                }
                (true, false) => Mode::Lossless,
                (false, true) => Mode::Lossy,
                (false, false) => Mode::Default,
            }
        };

        let quality = cli.quality.or(file.quality);
        let distance = cli.distance.or(file.distance);
        let optimize_alpha = cli.optimize_alpha || file.optimize_alpha.unwrap_or(false);

        if mode == Mode::Lossless {
            if quality.is_some() {
                return Err(Error::Invalid(
                    "--quality cannot be combined with --lossless (lossless mode never reduces quality)".into(),
                ));
            }
            if distance.is_some() {
                return Err(Error::Invalid("--distance cannot be combined with --lossless".into()));
            }
            if optimize_alpha {
                return Err(Error::Invalid(
                    "--optimize-alpha alters RGB values of transparent pixels and cannot be combined with --lossless"
                        .into(),
                ));
            }
        }
        if let Some(q) = quality {
            range("--quality", q, 0.0, 100.0)?;
        }
        if let Some(d) = distance {
            range("--distance", d, 0.0, 25.0)?;
            if mode == Mode::Lossy && d == 0.0 {
                return Err(Error::Invalid(
                    "--distance 0 means mathematically lossless in JPEG XL; omit --lossy or use --lossless".into(),
                ));
            }
        }
        let effort = cli.effort.or(file.effort);
        if let Some(e) = effort {
            range("--effort", e, 1, 11)?;
        }
        let brotli_effort = cli.brotli_effort.or(file.brotli_effort);
        if let Some(b) = brotli_effort {
            range("--brotli-effort", b, 0, 11)?;
        }
        let method = cli.method.or(file.method);
        if let Some(m) = method {
            range("--method", m, 0, 6)?;
        }
        let alpha_quality = cli.alpha_quality.or(file.alpha_quality);
        if let Some(a) = alpha_quality {
            range("--alpha-quality", a, 0, 100)?;
        }
        let png_preset = cli.png_preset.or(file.png_preset).unwrap_or(6);
        range("--png-preset", png_preset, 0, 6)?;
        let jobs = cli.jobs.or(file.jobs);
        let threads = cli.threads.or(file.threads);
        if jobs == Some(0) || threads == Some(0) {
            return Err(Error::Invalid("--jobs and --threads must be at least 1".into()));
        }

        let explicit_format = cli.format.or(file.format);
        let resize = match cli.resize.as_deref().or(file.resize.as_deref()) {
            Some(spec) => Some(spec.parse::<ResizeSpec>()?),
            None => None,
        };
        let resize_filter = cli.resize_filter.or(file.resize_filter).unwrap_or_default();

        Ok(Settings {
            input,
            output,
            format: explicit_format.unwrap_or(TargetFormat::Original),
            format_explicit: explicit_format.is_some(),
            mode,
            quality,
            distance,
            effort,
            brotli_effort,
            method,
            alpha_quality,
            sharp_yuv: cli.sharp_yuv || file.sharp_yuv.unwrap_or(false),
            jpeg_mode: cli.jpeg_mode.or(file.jpeg_mode).unwrap_or(JpegModeArg::Keep),
            png_preset,
            optimize_alpha,
            zopfli: !(cli.no_zopfli || file.zopfli == Some(false)),
            zopfli_iterations: cli.zopfli_iterations.or(file.zopfli_iterations).unwrap_or(DEFAULT_ZOPFLI_ITERATIONS),
            zopfli_no_improvement: Some(
                cli.zopfli_no_improvement.or(file.zopfli_no_improvement).unwrap_or(DEFAULT_ZOPFLI_NO_IMPROVEMENT),
            ),
            resize,
            resize_filter,
            strip_metadata: cli.strip_metadata || file.strip_metadata.unwrap_or(false),
            strip_icc: cli.strip_icc || file.strip_icc.unwrap_or(false),
            jobs,
            threads,
            overwrite: cli.overwrite || file.overwrite.unwrap_or(false),
            dry_run: cli.dry_run,
            recursive: cli.recursive || file.recursive.unwrap_or(false),
            keep_larger: cli.keep_larger || file.keep_larger.unwrap_or(false),
            report: cli.report || file.report.unwrap_or(false),
            verify: cli.verify.or(file.verify).unwrap_or(true),
            max_pixels: cli.max_pixels.or(file.max_pixels).unwrap_or(DEFAULT_MAX_PIXELS),
            quiet: cli.quiet,
            verbose: cli.verbose,
        })
    }
}
