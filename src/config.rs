//! Resolved run settings: CLI values > JSON configuration file > upstream tool defaults.

use crate::cli::{Cli, JpegModeArg, TargetFormat};
use crate::errors::{Error, Result};
use crate::image::resize::{ResizeFilter, ResizeSpec};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Upstream tool defaults per codec (`cjxl`, `cwebp`, `jpegoptim`, MozJPEG `cjpeg`, `oxipng`).
    Default,
    /// Strict lossless mode across codecs (`--lossless`).
    Lossless,
    /// Force lossy compression even where the upstream default is lossless (`--lossy`).
    Lossy,
}

/// Optional JSON configuration (keys mirror CLI options).
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub format: Option<TargetFormat>,
    pub lossless: Option<bool>,
    pub lossy: Option<bool>,
    pub max_compression: Option<bool>,
    pub quality: Option<f32>,
    pub distance: Option<f32>,
    pub effort: Option<i32>,
    pub allow_expert_options: Option<bool>,
    pub brotli_effort: Option<i32>,
    pub lossless_jpeg: Option<bool>,
    pub method: Option<i32>,
    pub lossless_preset: Option<i32>,
    pub alpha_quality: Option<i32>,
    pub exact: Option<bool>,
    pub sharp_yuv: Option<bool>,
    pub jpeg_mode: Option<JpegModeArg>,
    pub png_preset: Option<serde_json::Value>,
    pub optimize_alpha: Option<bool>,
    pub zopfli: Option<bool>,
    pub zopfli_iterations: Option<u64>,
    pub zopfli_no_improvement: Option<u64>,
    pub resize: Option<String>,
    pub resize_filter: Option<ResizeFilter>,
    pub strip_metadata: Option<bool>,
    pub strip_icc: Option<bool>,
    pub strip_all: Option<bool>,
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
    pub max_compression: bool,
    pub quality: Option<f32>,
    pub distance: Option<f32>,
    pub effort: Option<i32>,
    pub allow_expert_options: bool,
    pub brotli_effort: Option<i32>,
    pub lossless_jpeg: Option<bool>,
    pub method: Option<i32>,
    pub lossless_preset: Option<i32>,
    pub alpha_quality: Option<i32>,
    pub exact: bool,
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

// Upstream cjxl (libjxl) defaults & max-compression targets.
pub const DEFAULT_JXL_DISTANCE: f32 = 1.0; // cjxl -d 1.0 (visually lossless)
pub const DEFAULT_JXL_EFFORT: i32 = 7; // cjxl -e 7
pub const DEFAULT_JXL_BROTLI_EFFORT: i32 = 9; // cjxl --brotli_effort 9
pub const MAX_JXL_EFFORT_LOSSY: i32 = 10;
pub const MAX_JXL_EFFORT_LOSSLESS: i32 = 11; // requires expert options
pub const MAX_JXL_BROTLI_EFFORT: i32 = 11;

// Upstream cwebp (libwebp) defaults & max-compression targets.
pub const DEFAULT_WEBP_QUALITY: f32 = 75.0; // cwebp -q 75
pub const DEFAULT_WEBP_METHOD: i32 = 4; // cwebp -m 4
pub const MAX_WEBP_METHOD: i32 = 6; // cwebp -m 6

// Upstream MozJPEG (cjpeg) default quality.
pub const DEFAULT_JPEG_QUALITY: f32 = 75.0; // cjpeg -quality 75

// Upstream oxipng defaults & max-compression targets.
pub const DEFAULT_PNG_PRESET: u8 = 2; // oxipng -o 2
pub const MAX_PNG_PRESET: u8 = 6; // oxipng -o max
pub const DEFAULT_ZOPFLI_ITERATIONS: u64 = 15; // oxipng --zi 15
pub const MAX_ZOPFLI_ITERATIONS: u64 = 60;
pub const MAX_ZOPFLI_NO_IMPROVEMENT: u64 = 15;

fn range<T: PartialOrd + std::fmt::Display>(name: &str, v: T, lo: T, hi: T) -> Result<()> {
    if v < lo || v > hi {
        return Err(Error::Invalid(format!("{name} must be between {lo} and {hi} (got {v})")));
    }
    Ok(())
}

fn parse_png_preset_str(raw: &str) -> Result<u8> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("max") {
        return Ok(MAX_PNG_PRESET);
    }
    let n: u8 = s
        .parse()
        .map_err(|_| Error::Invalid(format!("--opt/--png-preset must be 0-6 or 'max' (got '{raw}')")))?;
    range("--opt/--png-preset", n, 0, MAX_PNG_PRESET)?;
    Ok(n)
}

fn parse_png_preset_json(v: &serde_json::Value) -> Result<u8> {
    match v {
        serde_json::Value::Number(n) => {
            let u = n
                .as_u64()
                .ok_or_else(|| Error::Invalid(format!("png_preset must be 0-6 or 'max' (got {v})")))?;
            if u > MAX_PNG_PRESET as u64 {
                return Err(Error::Invalid(format!("png_preset must be between 0 and 6 (got {u})")));
            }
            Ok(u as u8)
        }
        serde_json::Value::String(s) => parse_png_preset_str(s),
        _ => Err(Error::Invalid(format!("png_preset must be 0-6 or 'max' (got {v})"))),
    }
}

fn resolve_mode(cli: &Cli, file: &FileConfig) -> Result<Mode> {
    if cli.lossless {
        Ok(Mode::Lossless)
    } else if cli.lossy {
        Ok(Mode::Lossy)
    } else {
        match (file.lossless.unwrap_or(false), file.lossy.unwrap_or(false)) {
            (true, true) => Err(Error::Invalid("configuration file cannot set both 'lossless' and 'lossy' to true".into())),
            (true, false) => Ok(Mode::Lossless),
            (false, true) => Ok(Mode::Lossy),
            (false, false) => Ok(Mode::Default),
        }
    }
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

        let mode = resolve_mode(cli, &file)?;
        let input = cli
            .input
            .clone()
            .or_else(|| cli.input_pos.clone())
            .or(file.input)
            .ok_or_else(|| Error::Invalid("INPUT path (or --input) is required".into()))?;
        let output = cli
            .output
            .clone()
            .or_else(|| cli.output_pos.clone())
            .or(file.output)
            .ok_or_else(|| Error::Invalid("OUTPUT path (or --output) is required".into()))?;
        let max_compression = cli.max_compression || file.max_compression.unwrap_or(false);
        let quality = cli.quality.or(file.quality);
        let distance = cli.distance.or(file.distance);
        let effort = cli.effort.or(file.effort);
        let allow_expert_options = cli.allow_expert_options || file.allow_expert_options.unwrap_or(false) || max_compression;
        let brotli_effort = cli.brotli_effort.or(file.brotli_effort);
        let lossless_jpeg = match cli.lossless_jpeg {
            Some(0) => Some(false),
            Some(1) => Some(true),
            Some(v) => return Err(Error::Invalid(format!("--lossless_jpeg must be 0 or 1 (got {v})"))),
            None => file.lossless_jpeg,
        };
        let method = cli.method.or(file.method);
        let lossless_preset = cli.lossless_preset.or(file.lossless_preset);
        let alpha_quality = cli.alpha_quality.or(file.alpha_quality);
        let optimize_alpha = cli.optimize_alpha || file.optimize_alpha.unwrap_or(false);

        // ---- range and compatibility validation
        if let Some(q) = quality {
            range("--quality", q, 0.0, 100.0)?;
        }
        if let Some(d) = distance {
            range("--distance", d, 0.0, 25.0)?;
            if mode == Mode::Lossless && d > 0.0 {
                return Err(Error::Invalid("--distance > 0 cannot be combined with --lossless".into()));
            }
            if mode == Mode::Lossy && d == 0.0 {
                return Err(Error::Invalid(
                    "--distance 0 means mathematically lossless in JPEG XL; omit --lossy or use --lossless".into(),
                ));
            }
        }
        if let Some(e) = effort {
            range("--effort", e, 1, 11)?;
            if e > 10 && !allow_expert_options {
                return Err(Error::Invalid(
                    "--effort 11 requires --allow_expert_options (matches cjxl behavior)".into(),
                ));
            }
        }
        if let Some(b) = brotli_effort {
            range("--brotli_effort", b, 0, 11)?;
        }
        if let Some(m) = method {
            range("--method", m, 0, 6)?;
        }
        if let Some(z) = lossless_preset {
            range("--lossless-preset", z, 0, 9)?;
            if mode == Mode::Lossy {
                return Err(Error::Invalid("--lossless-preset cannot be combined with --lossy".into()));
            }
        }
        if let Some(a) = alpha_quality {
            range("--alpha_q", a, 0, 100)?;
        }
        if mode == Mode::Lossless && optimize_alpha {
            return Err(Error::Invalid(
                "--alpha / --optimize-alpha alters RGB values of transparent pixels and cannot be combined with --lossless"
                    .into(),
            ));
        }

        let explicit_format = cli.format.or(file.format);
        if mode == Mode::Lossless {
            if let Some(q) = quality {
                match explicit_format {
                    Some(TargetFormat::Jpeg) => {
                        return Err(Error::Invalid("--quality cannot be combined with --lossless for JPEG".into()));
                    }
                    Some(TargetFormat::Jxl) if q < 100.0 => {
                        return Err(Error::Invalid("--quality < 100 cannot be combined with --lossless for JPEG XL".into()));
                    }
                    _ => {}
                }
            }
        }

        let png_preset = if let Some(s) = &cli.png_preset {
            parse_png_preset_str(s)?
        } else if let Some(v) = &file.png_preset {
            parse_png_preset_json(v)?
        } else if max_compression {
            MAX_PNG_PRESET
        } else {
            DEFAULT_PNG_PRESET
        };

        let zopfli_hint = cli.zopfli || cli.zopfli_iterations.is_some() || cli.zopfli_no_improvement.is_some();
        let zopfli = if cli.no_zopfli {
            false
        } else if zopfli_hint {
            true
        } else if let Some(fz) = file.zopfli {
            fz
        } else {
            max_compression
        };
        let zopfli_iterations = cli
            .zopfli_iterations
            .or(file.zopfli_iterations)
            .unwrap_or(if max_compression { MAX_ZOPFLI_ITERATIONS } else { DEFAULT_ZOPFLI_ITERATIONS });
        let zopfli_no_improvement = cli
            .zopfli_no_improvement
            .or(file.zopfli_no_improvement)
            .or(if max_compression { Some(MAX_ZOPFLI_NO_IMPROVEMENT) } else { None });

        let jpeg_mode = if cli.all_progressive {
            JpegModeArg::Progressive
        } else if cli.all_normal {
            JpegModeArg::Baseline
        } else if let Some(m) = cli.jpeg_mode.or(file.jpeg_mode) {
            m
        } else if max_compression {
            JpegModeArg::Auto
        } else {
            JpegModeArg::Keep
        };

        let jobs = cli.jobs.or(file.jobs);
        let threads = cli.threads.or(file.threads);
        if jobs == Some(0) || threads == Some(0) {
            return Err(Error::Invalid("--jobs and --threads must be at least 1".into()));
        }

        let resize = match cli.resize.as_deref().or(file.resize.as_deref()) {
            Some(spec) => Some(spec.parse::<ResizeSpec>()?),
            None => None,
        };
        let resize_filter = cli.resize_filter.or(file.resize_filter).unwrap_or_default();

        let strip_all = cli.strip_all || file.strip_all.unwrap_or(false);
        let strip_metadata = strip_all || cli.strip_metadata || file.strip_metadata.unwrap_or(false);
        let strip_icc = strip_all || cli.strip_icc || file.strip_icc.unwrap_or(false);

        Ok(Settings {
            input,
            output,
            format: explicit_format.unwrap_or(TargetFormat::Original),
            format_explicit: explicit_format.is_some(),
            mode,
            max_compression,
            quality,
            distance,
            effort,
            allow_expert_options,
            brotli_effort,
            lossless_jpeg,
            method,
            lossless_preset,
            alpha_quality,
            exact: cli.exact || file.exact.unwrap_or(false),
            sharp_yuv: cli.sharp_yuv || file.sharp_yuv.unwrap_or(false),
            jpeg_mode,
            png_preset,
            optimize_alpha,
            zopfli,
            zopfli_iterations,
            zopfli_no_improvement,
            resize,
            resize_filter,
            strip_metadata,
            strip_icc,
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
