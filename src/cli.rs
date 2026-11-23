//! Command line interface.
//!
//! Canonical invocation: `ImageOptimizer [OPTIONS] <INPUT> <OUTPUT>` (options may also appear
//! after positional paths, and `-i`/`-o` remain supported for directory/config workflows).
//! Every codec defaults to the exact behavior of the upstream tool it wraps (`cjxl`, `cwebp`,
//! `jpegoptim`, MozJPEG `cjpeg`, `oxipng`) and accepts its standard flags.

use crate::image::resize::ResizeFilter;
use clap::{ArgAction, Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetFormat {
    /// JPEG XL (libjxl / cjxl)
    Jxl,
    /// WebP (libwebp / cwebp)
    Webp,
    /// JPEG (jpegoptim core for JPEG sources, MozJPEG for conversions)
    Jpeg,
    /// PNG (OxiPNG + Zopfli)
    Png,
    /// Optimize every file in its existing format
    Original,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JpegModeArg {
    /// Keep the progressive/baseline mode of each JPEG input (jpegoptim default); conversions use progressive
    Keep,
    /// Force progressive scan coding
    Progressive,
    /// Force baseline scan coding
    Baseline,
    /// Try both baseline and progressive for JPEG -> JPEG and keep the smaller output
    Auto,
}

#[derive(Parser, Debug)]
#[command(
    name = "ImageOptimizer",
    override_usage = "ImageOptimizer [OPTIONS] <INPUT> <OUTPUT>",
    about = "Optimize and convert images with libjxl (cjxl), libwebp (cwebp), jpegoptim (MozJPEG) and OxiPNG in one executable.",
    long_about = "Optimize and convert images with libjxl (cjxl), libwebp (cwebp), jpegoptim (MozJPEG) and OxiPNG in one executable.\n\nEach output format follows the default behavior and flags of its upstream tool:\n  JXL  (cjxl)      : d=1.0, e=7, brotli_effort=9 (lossless JPEG transcode for JPEGs)\n  WebP (cwebp)     : lossy q=75, m=4, alpha_q=100\n  JPEG (jpegoptim) : lossless Huffman for JPEGs; MozJPEG cjpeg q=75 progressive/trellis for conversions\n  PNG  (oxipng)    : lossless --opt 2 (libdeflate); add -z/--zopfli for Zopfli\n\nUse --lossless for strict lossless mode and --max-compression for maximum effort across all codecs. Source files are never modified.",
    disable_version_flag = true,
    after_help = "EXAMPLES:\n  ImageOptimizer --distance=0 --effort=11 --allow_expert_options --brotli_effort=11 test.png out.jxl\n  ImageOptimizer --lossless --max-compression test.png out.jxl\n  ImageOptimizer -q 80 -m 6 --sharp_yuv test.png out.webp\n  ImageOptimizer --lossless -q 100 -m 6 --exact test.png out.webp\n  ImageOptimizer --opt max -z test.png out.png\n  ImageOptimizer --max=85 --all-progressive test.jpg out.jpg\n  ImageOptimizer --resize 256 --lossless --max-compression test.png thumb.jxl\n  ImageOptimizer --format jxl --recursive --input D:\\Images --output D:\\Output"
)]
pub struct Cli {
    /// Input file or directory
    #[arg(value_name = "INPUT", conflicts_with = "input")]
    pub input_pos: Option<PathBuf>,
    /// Output file (for a single input file) or directory
    #[arg(value_name = "OUTPUT", conflicts_with = "output")]
    pub output_pos: Option<PathBuf>,

    /// Input file or directory (named alternative to positional INPUT)
    #[arg(short, long, value_name = "PATH")]
    pub input: Option<PathBuf>,
    /// Output file or directory (named alternative to positional OUTPUT)
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,
    /// Target format (inferred from OUTPUT extension in single-file mode; default: original for directories)
    #[arg(short, long, value_enum)]
    pub format: Option<TargetFormat>,

    /// Force mathematically lossless mode across codecs (refuses inherently lossy routes)
    #[arg(long, conflicts_with = "lossy")]
    pub lossless: bool,
    /// Force lossy compression even where the upstream default is lossless (JPEG->JPEG, JPEG->JXL)
    #[arg(long, visible_alias = "allow-lossy")]
    pub lossy: bool,
    /// Maximum compression preset across all codecs (JXL e=11/10 + brotli 11, WebP m=6, PNG opt=6 + Zopfli, JPEG auto)
    #[arg(long, alias = "best")]
    pub max_compression: bool,

    // ---- Shared quality
    /// Codec-specific quality 0-100 (cwebp default 75, MozJPEG default 75, jpegoptim --max, cjxl mapped to distance)
    #[arg(short, long, visible_alias = "max", value_name = "0-100")]
    pub quality: Option<f32>,

    // ---- JPEG XL (cjxl)
    /// JPEG XL Butteraugli distance 0-25 (cjxl default: 1.0; 0 = lossless; overrides --quality)
    #[arg(short, long, value_name = "D", help_heading = "JPEG XL (cjxl)")]
    pub distance: Option<f32>,
    /// JPEG XL effort 1-11 (cjxl default: 7; 11 requires --allow_expert_options)
    #[arg(short, long, value_name = "1-11", help_heading = "JPEG XL (cjxl)")]
    pub effort: Option<i32>,
    /// Unlock libjxl expert options such as --effort=11 (matches cjxl --allow_expert_options)
    #[arg(long = "allow_expert_options", alias = "allow-expert-options", help_heading = "JPEG XL (cjxl)")]
    pub allow_expert_options: bool,
    /// JPEG XL Brotli effort 0-11 for metadata boxes and JPEG transcoding (cjxl default: 9)
    #[arg(long = "brotli_effort", alias = "brotli-effort", value_name = "0-11", help_heading = "JPEG XL (cjxl)")]
    pub brotli_effort: Option<i32>,
    /// JPEG -> JXL lossless bitstream transcoding: 1 = transcode (cjxl default), 0 = decode & re-encode
    #[arg(long = "lossless_jpeg", alias = "lossless-jpeg", value_name = "0|1", help_heading = "JPEG XL (cjxl)")]
    pub lossless_jpeg: Option<u8>,

    // ---- WebP (cwebp)
    /// WebP compression method 0-6 (cwebp default: 4)
    #[arg(short, long, value_name = "0-6", help_heading = "WebP (cwebp)")]
    pub method: Option<i32>,
    /// WebP lossless preset 0-9 (cwebp -z: activates lossless mode and sets effort preset)
    #[arg(long, alias = "webp-z", value_name = "0-9", conflicts_with = "lossy", help_heading = "WebP (cwebp)")]
    pub lossless_preset: Option<i32>,
    /// WebP alpha-channel quality 0-100 (cwebp default: 100)
    #[arg(long = "alpha_q", visible_alias = "alpha-quality", value_name = "0-100", help_heading = "WebP (cwebp)")]
    pub alpha_quality: Option<i32>,
    /// Preserve exact RGB values under fully transparent pixels in lossless WebP (cwebp -exact)
    #[arg(long, help_heading = "WebP (cwebp)")]
    pub exact: bool,
    /// Sharp RGB->YUV conversion for lossy WebP (cwebp -sharp_yuv)
    #[arg(long = "sharp_yuv", visible_alias = "sharp-yuv", help_heading = "WebP (cwebp)")]
    pub sharp_yuv: bool,

    // ---- JPEG (jpegoptim / MozJPEG)
    /// JPEG scan mode (keep, progressive, baseline, auto)
    #[arg(long, value_enum, conflicts_with_all = ["all_progressive", "all_normal"], help_heading = "JPEG (jpegoptim / MozJPEG)")]
    pub jpeg_mode: Option<JpegModeArg>,
    /// Force progressive JPEG output (jpegoptim --all-progressive)
    #[arg(long, conflicts_with = "all_normal", help_heading = "JPEG (jpegoptim / MozJPEG)")]
    pub all_progressive: bool,
    /// Force baseline JPEG output (jpegoptim --all-normal)
    #[arg(long, help_heading = "JPEG (jpegoptim / MozJPEG)")]
    pub all_normal: bool,

    // ---- PNG (oxipng)
    /// OxiPNG optimization preset 0-6 or max (oxipng default: 2)
    #[arg(long = "opt", visible_alias = "png-preset", value_name = "0-6|max", help_heading = "PNG (oxipng)")]
    pub png_preset: Option<String>,
    /// Use Zopfli deflate for PNG (oxipng -z / --zopfli; slower, smaller)
    #[arg(short = 'z', long, conflicts_with = "no_zopfli", help_heading = "PNG (oxipng)")]
    pub zopfli: bool,
    /// Disable Zopfli and use libdeflate for PNG (default unless -z or --max-compression is set)
    #[arg(long, help_heading = "PNG (oxipng)")]
    pub no_zopfli: bool,
    /// Zopfli iteration count (oxipng --zi; default: 15, or 60 with --max-compression)
    #[arg(long = "zi", visible_alias = "zopfli-iterations", value_name = "N", help_heading = "PNG (oxipng)")]
    pub zopfli_iterations: Option<u64>,
    /// Stop Zopfli after N iterations without improvement (oxipng --ziwi)
    #[arg(long = "ziwi", visible_alias = "zopfli-no-improvement", value_name = "N", help_heading = "PNG (oxipng)")]
    pub zopfli_no_improvement: Option<u64>,
    /// Optimize RGB values of fully transparent PNG pixels (oxipng -a / --alpha; disallowed with --lossless)
    #[arg(short = 'a', long = "alpha", visible_alias = "optimize-alpha", help_heading = "PNG (oxipng)")]
    pub optimize_alpha: bool,

    // ---- Resize
    /// Downscale only to fit within WxH (e.g. 256, 256x256, 256x, x256, 256x256!); never upscales
    #[arg(long, value_name = "WxH", help_heading = "Resize")]
    pub resize: Option<String>,
    /// Resampling filter used when downscaling (default: lanczos3)
    #[arg(long, value_enum, help_heading = "Resize")]
    pub resize_filter: Option<ResizeFilter>,

    // ---- Metadata & execution
    /// Remove EXIF, XMP, IPTC and comments (ICC profiles are kept)
    #[arg(long, help_heading = "Metadata & Execution")]
    pub strip_metadata: bool,
    /// Also remove ICC profiles (changes how colours are displayed)
    #[arg(long, help_heading = "Metadata & Execution")]
    pub strip_icc: bool,
    /// Remove all metadata including ICC profiles (jpegoptim --strip-all; combines --strip-metadata + --strip-icc)
    #[arg(long, help_heading = "Metadata & Execution")]
    pub strip_all: bool,
    /// Number of files processed in parallel
    #[arg(short, long, value_name = "N", help_heading = "Metadata & Execution")]
    pub jobs: Option<usize>,
    /// Codec-internal threads per file
    #[arg(short, long, value_name = "N", help_heading = "Metadata & Execution")]
    pub threads: Option<usize>,
    /// Replace existing output files
    #[arg(long, help_heading = "Metadata & Execution")]
    pub overwrite: bool,
    /// Show what would be done without writing anything
    #[arg(long, help_heading = "Metadata & Execution")]
    pub dry_run: bool,
    /// Scan input directories recursively
    #[arg(short, long, help_heading = "Metadata & Execution")]
    pub recursive: bool,
    /// Write optimized output even when it is not smaller than the source
    #[arg(long, help_heading = "Metadata & Execution")]
    pub keep_larger: bool,
    /// Write Reports/report.csv and report.json (disabled by default)
    #[arg(long, help_heading = "Metadata & Execution")]
    pub report: bool,
    /// Verify every result (default: on). Use --verify=false to disable.
    #[arg(long, action = ArgAction::Set, num_args = 0..=1, require_equals = true, default_missing_value = "true", value_name = "BOOL", help_heading = "Metadata & Execution")]
    pub verify: Option<bool>,
    /// Refuse images with more pixels than this (default 268435456)
    #[arg(long, value_name = "N", help_heading = "Metadata & Execution")]
    pub max_pixels: Option<u64>,
    /// JSON configuration file (command line values take precedence)
    #[arg(long, value_name = "FILE", help_heading = "Metadata & Execution")]
    pub config: Option<PathBuf>,
    /// Less console output
    #[arg(long, help_heading = "Metadata & Execution")]
    pub quiet: bool,
    /// More log output (-v, -vv)
    #[arg(short, long, action = ArgAction::Count, help_heading = "Metadata & Execution")]
    pub verbose: u8,
    /// List supported input/output formats and exit
    #[arg(long)]
    pub list_formats: bool,
    /// Print version and codec versions, then exit
    #[arg(short = 'V', long)]
    pub version: bool,
}
