//! Command line interface. Every option is optional here: values are merged with an optional
//! JSON configuration file and the built-in defaults in `config::Settings::resolve`.

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
    about = "Optimize and convert images with libjxl, libwebp, jpegoptim (MozJPEG) and OxiPNG (Zopfli) in one executable.",
    long_about = "Optimize and convert images with libjxl (cjxl), libwebp (cwebp), jpegoptim (MozJPEG) and OxiPNG (Zopfli) in one executable.\n\nAccepts either a single input/output file pair (inferring the target format from the output extension when --format is omitted) or an input/output directory. By default each codec follows its upstream tool's default mode (lossy for WebP, non-JPEG->JXL and non-JPEG->JPEG; lossless for PNG, JPEG->JPEG and JPEG->JXL), tuned for maximum compression effort. Pass --lossless for strict mathematically lossless output across all codecs. Source files are never modified.",
    disable_version_flag = true,
    after_help = "EXAMPLES:\n  ImageOptimizer photo.png photo.webp\n  ImageOptimizer photo.png photo.jxl --lossless\n  ImageOptimizer photo.png thumb.webp --resize 256x256\n  ImageOptimizer -i photo.png -o photo.jpg -q 85\n  ImageOptimizer --input D:\\Images --format jxl --output D:\\Output\n  ImageOptimizer --input D:\\Images --format original --lossless --recursive --output D:\\Output"
)]
pub struct Cli {
    /// Input file or directory (positional shorthand for --input)
    #[arg(value_name = "INPUT", conflicts_with = "input")]
    pub input_pos: Option<PathBuf>,
    /// Output file or directory (positional shorthand for --output)
    #[arg(value_name = "OUTPUT", conflicts_with = "output")]
    pub output_pos: Option<PathBuf>,

    /// Input file or directory
    #[arg(short, long, value_name = "PATH")]
    pub input: Option<PathBuf>,
    /// Output file (for a single input file) or directory
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,
    /// Target format (inferred from the output file extension in single-file mode; default: original for directories)
    #[arg(short, long, value_enum)]
    pub format: Option<TargetFormat>,

    /// Force strict mathematically lossless mode across all codecs (refuses inherently lossy routes)
    #[arg(long, conflicts_with = "lossy")]
    pub lossless: bool,
    /// Force lossy compression even on routes whose upstream default is lossless (JPEG->JPEG, JPEG->JXL)
    #[arg(long, visible_alias = "allow-lossy")]
    pub lossy: bool,

    /// Codec-specific quality 0-100 (cwebp default 75, MozJPEG default 75, cjxl mapped to distance; NOT interchangeable between codecs)
    #[arg(short, long, value_name = "0-100")]
    pub quality: Option<f32>,
    /// JPEG XL Butteraugli distance 0-25 (0 = lossless, default 1.0; overrides --quality for JPEG XL)
    #[arg(long, value_name = "D")]
    pub distance: Option<f32>,
    /// JPEG XL effort 1-11 (default: 9 lossy, 11 lossless with expert options)
    #[arg(long, value_name = "1-11")]
    pub effort: Option<i32>,
    /// JPEG XL Brotli compression effort 0-11 (default: 11, maximum compression)
    #[arg(long, value_name = "0-11")]
    pub brotli_effort: Option<i32>,
    /// WebP compression method 0-6 (default: 6, maximum compression)
    #[arg(long, value_name = "0-6")]
    pub method: Option<i32>,
    /// WebP alpha-channel quality 0-100 (default: 100)
    #[arg(long, value_name = "0-100")]
    pub alpha_quality: Option<i32>,
    /// Use sharp RGB->YUV conversion for lossy WebP (cwebp -sharp_yuv)
    #[arg(long)]
    pub sharp_yuv: bool,
    /// JPEG scan mode for JPEG output
    #[arg(long, value_enum)]
    pub jpeg_mode: Option<JpegModeArg>,
    /// OxiPNG preset 0-6 (default: 6, maximum compression)
    #[arg(long, value_name = "0-6")]
    pub png_preset: Option<u8>,
    /// Optimize RGB values of fully transparent PNG pixels (oxipng --alpha; visually lossless, disallowed with --lossless)
    #[arg(long)]
    pub optimize_alpha: bool,
    /// Use libdeflate instead of Zopfli for PNG (faster, larger)
    #[arg(long)]
    pub no_zopfli: bool,
    /// Zopfli iteration count
    #[arg(long, value_name = "N")]
    pub zopfli_iterations: Option<u64>,
    /// Stop Zopfli after N iterations without improvement
    #[arg(long, value_name = "N")]
    pub zopfli_no_improvement: Option<u64>,

    /// Downscale only to fit within WxH (e.g. 256x256, 256x, x256, 256x256!); never upscales
    #[arg(long, value_name = "WxH")]
    pub resize: Option<String>,
    /// Resampling filter used when downscaling (default: lanczos3)
    #[arg(long, value_enum)]
    pub resize_filter: Option<ResizeFilter>,

    /// Remove EXIF, XMP, IPTC and comments (ICC profiles are kept)
    #[arg(long)]
    pub strip_metadata: bool,
    /// Also remove ICC profiles (changes how colours are displayed)
    #[arg(long)]
    pub strip_icc: bool,

    /// Number of files processed in parallel
    #[arg(short, long, value_name = "N")]
    pub jobs: Option<usize>,
    /// Codec-internal threads per file
    #[arg(short, long, value_name = "N")]
    pub threads: Option<usize>,

    /// Replace existing output files
    #[arg(long)]
    pub overwrite: bool,
    /// Show what would be done without writing anything
    #[arg(long)]
    pub dry_run: bool,
    /// Scan input directories recursively
    #[arg(short, long)]
    pub recursive: bool,
    /// Write optimized output even when it is not smaller than the source
    #[arg(long)]
    pub keep_larger: bool,
    /// Write Reports/report.csv and report.json (disabled by default)
    #[arg(long)]
    pub report: bool,
    /// Verify every result (default: on). Use --verify=false to disable.
    #[arg(long, action = ArgAction::Set, num_args = 0..=1, require_equals = true, default_missing_value = "true", value_name = "BOOL")]
    pub verify: Option<bool>,
    /// Refuse images with more pixels than this (default 268435456)
    #[arg(long, value_name = "N")]
    pub max_pixels: Option<u64>,

    /// JSON configuration file (command line values take precedence)
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    /// Less console output
    #[arg(long)]
    pub quiet: bool,
    /// More log output (-v, -vv)
    #[arg(short, long, action = ArgAction::Count)]
    pub verbose: u8,

    /// List supported input/output formats and exit
    #[arg(long)]
    pub list_formats: bool,
    /// Print version and codec versions, then exit
    #[arg(short = 'V', long)]
    pub version: bool,
}
