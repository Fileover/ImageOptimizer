//! Application driver: configuration, discovery, scheduling, progress, optional reports, exit codes.

use crate::cli::{Cli, TargetFormat};
use crate::config::{Mode, Settings};
use crate::engines::{libjxl, libwebp, mozjpeg, oxipng};
use crate::errors::{Error, Result};
use crate::image::identification::{identify, Format};
use crate::logging;
use crate::processing::pipeline::resolve_target;
use crate::processing::scheduler;
use crate::processing::{process_file, FileResult, Status};
use crate::reporting::{self, RunInfo, Summary};
use crate::utils::filesystem::{long_path, scan_inputs, InputFile, OutputPlanner};
use crate::utils::formatting::{hms, human_bytes};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const APP_NAME: &str = "ImageOptimizer";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const EXIT_OK: i32 = 0;
pub const EXIT_FILE_FAILURES: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_CANCELLED: i32 = 130;

pub fn codec_versions() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("libjxl (encoder)".into(), libjxl::encoder_version());
    m.insert("libjxl (decoder)".into(), libjxl::decoder_version());
    m.insert("libwebp (encoder)".into(), libwebp::version());
    m.insert("mozjpeg".into(), mozjpeg::version());
    m.insert("jpegoptim (core)".into(), "1.5.6".into());
    m.insert("oxipng".into(), oxipng::version());
    m
}

pub fn version_text() -> String {
    let mut s = format!("{APP_NAME} {APP_VERSION}\n");
    for (k, v) in codec_versions() {
        s.push_str(&format!("  {k}: {v}\n"));
    }
    s.push_str("License: GPL-3.0-or-later (combined work; contains code adapted from jpegoptim). See THIRD-PARTY-NOTICES.md.\n");
    s
}

pub fn formats_text() -> &'static str {
    "Input formats (detected by file signature, not by extension):\n  PNG (incl. APNG, optimized in place only)\n  JPEG (baseline/progressive/arithmetic decode)\n  WebP (still images; animated WebP is refused)\n  JPEG XL (still images; animated JPEG XL is refused)\n  BMP, TIFF, GIF (GIF: single-frame only; animated GIF is refused)\n\nOutput formats (--format) and upstream defaults (all tuned for maximum optimization; add --lossless for strict lossless):\n  jxl       JPEG XL via libjxl (cjxl defaults: lossless JPEG transcode for JPEGs, lossy d=1.0 for others; effort 9 lossy / 11 lossless, brotli_effort 11)\n  webp      WebP via libwebp (cwebp default: lossy q=75, method 6; --lossless for lossless q=100 m=6 exact=1)\n  jpeg      JPEG: jpegoptim core (lossless default, lossy with --quality/--lossy) for JPEGs; MozJPEG (q=75, progressive, trellis) for conversions\n  png       PNG via OxiPNG preset 6 + Zopfli (lossless; --optimize-alpha for transparent RGB optimization)\n  original  optimize each file in its own format (PNG, JPEG, WebP, JPEG XL)\n"
}

struct Job {
    input: InputFile,
    dest: PathBuf,
}

/// Maps a recognized output file extension to a [`TargetFormat`].
fn target_format_from_extension(p: &Path) -> Option<TargetFormat> {
    match p.extension()?.to_string_lossy().to_lowercase().as_str() {
        "png" => Some(TargetFormat::Png),
        "jpg" | "jpeg" | "jpe" | "jfif" => Some(TargetFormat::Jpeg),
        "webp" => Some(TargetFormat::Webp),
        "jxl" => Some(TargetFormat::Jxl),
        _ => None,
    }
}

/// True when `--input` is a single file and `--output` designates an exact output file path
/// (rather than an output directory).
fn is_single_file_output(input: &Path, output: &Path) -> bool {
    if !input.is_file() || output.is_dir() {
        return false;
    }
    let raw = output.to_string_lossy();
    if raw.ends_with('/') || raw.ends_with('\\') {
        return false;
    }
    output.is_file() || output.extension().is_some()
}

/// Reads up to 32 bytes and identifies the format by signature.
fn sniff(p: &Path) -> Option<Format> {
    let mut f = std::fs::File::open(long_path(p)).ok()?;
    let mut buf = [0u8; 32];
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(_) => return None,
        }
    }
    identify(&buf[..n])
}

pub fn run(cli: &Cli) -> i32 {
    if cli.version {
        print!("{}", version_text());
        return EXIT_OK;
    }
    if cli.list_formats {
        print!("{}", formats_text());
        return EXIT_OK;
    }
    logging::init(cli.verbose, cli.quiet);
    let settings = match Settings::resolve(cli) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_USAGE;
        }
    };
    match execute(settings) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_USAGE
        }
    }
}

fn execute(mut settings: Settings) -> Result<i32> {
    let started = Instant::now();
    let started_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);

    if !settings.input.exists() {
        return Err(Error::Invalid(format!("input does not exist: {}", settings.input.display())));
    }
    let single_file = settings.input.is_file();
    if !single_file && settings.output.is_file() {
        return Err(Error::Invalid(format!(
            "input is a directory, so --output must be a directory (got existing file: {})",
            settings.output.display()
        )));
    }
    let single_file_output = is_single_file_output(&settings.input, &settings.output);

    if single_file_output && !settings.format_explicit {
        if let Some(inferred) = target_format_from_extension(&settings.output) {
            settings.format = inferred;
        } else if let Some(ext) = settings.output.extension() {
            return Err(Error::Invalid(format!(
                "unsupported output format extension '.{}' in {} (supported: .png, .jpg, .jpeg, .webp, .jxl; or pass --format)",
                ext.to_string_lossy(),
                settings.output.display()
            )));
        }
    }

    if !settings.dry_run {
        if single_file_output {
            if let Some(parent) = settings.output.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(long_path(parent)).map_err(|e| Error::io(parent, e))?;
            }
        } else {
            std::fs::create_dir_all(long_path(&settings.output)).map_err(|e| Error::io(&settings.output, e))?;
        }
    }

    // ---- discovery
    let candidates = scan_inputs(&settings.input, settings.recursive, &settings.output)?;
    let total_candidates = candidates.len();
    let mut planner = OutputPlanner::default();
    let mut jobs_list = Vec::new();
    let mut ignored = 0usize;
    for c in candidates {
        // Files without an image signature are ignored silently in directories, but reported when named explicitly.
        let Some(src) = sniff(&c.path) else {
            if single_file {
                let dest = if single_file_output { settings.output.clone() } else { PathBuf::new() };
                jobs_list.push(Job { dest, input: c });
            } else {
                tracing::debug!("ignored (no image signature): {}", c.path.display());
                ignored += 1;
            }
            continue;
        };
        let dest = if single_file_output {
            settings.output.clone()
        } else {
            let dst = resolve_target(&settings, src).unwrap_or(src);
            let dir = settings.output.join(dst.output_dir()).join(&c.rel_dir);
            let ext = if src == dst {
                c.path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .filter(|e| !e.is_empty())
                    .unwrap_or_else(|| dst.extension().to_string())
            } else {
                dst.extension().to_string()
            };
            planner.plan(&dir, &c.path, &ext)
        };
        jobs_list.push(Job { input: c, dest });
    }
    tracing::info!("{} candidate files, {} image jobs, {} ignored", total_candidates, jobs_list.len(), ignored);

    if jobs_list.is_empty() {
        println!("No supported image files found in {} ({} files ignored).", settings.input.display(), ignored);
        return Ok(EXIT_OK);
    }

    // ---- scheduling
    let cpus = scheduler::logical_cpus();
    let plan = scheduler::plan(jobs_list.len(), settings.jobs, settings.threads, cpus, scheduler::available_memory_mb());
    if plan.oversubscribed {
        tracing::warn!(
            "jobs ({}) x threads ({}) exceeds the {} logical processors: the machine will be oversubscribed",
            plan.jobs,
            plan.threads,
            plan.cpus
        );
    }
    if !settings.quiet && !single_file_output {
        println!(
            "{APP_NAME} {APP_VERSION} | mode: {} | format: {:?} | {} files | jobs: {} x threads: {} ({} logical CPUs){}",
            match settings.mode {
                Mode::Default => "default (upstream)",
                Mode::Lossless => "lossless",
                Mode::Lossy => "LOSSY",
            },
            settings.format,
            jobs_list.len(),
            plan.jobs,
            plan.threads,
            plan.cpus,
            if settings.dry_run { " | DRY RUN" } else { "" }
        );
    }

    // ---- cancellation
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let c = cancel.clone();
        let _ = ctrlc::set_handler(move || {
            if c.swap(true, Ordering::SeqCst) {
                std::process::exit(EXIT_CANCELLED);
            }
            eprintln!("\nCancelling: finishing the files in progress (press Ctrl+C again to abort immediately)...");
        });
    }

    // ---- processing
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(plan.jobs)
        .thread_name(|i| format!("worker-{i}"))
        .build()
        .map_err(|e| Error::Invalid(format!("cannot create thread pool: {e}")))?;
    let done = AtomicUsize::new(0);
    let total = jobs_list.len();
    let print_lock = Mutex::new(());
    let threads = plan.threads as i32;
    let results: Vec<FileResult> = pool.install(|| {
        jobs_list
            .par_iter()
            .map(|job| {
                if cancel.load(Ordering::Relaxed) {
                    let mut r = process_cancelled(&settings, &job.input);
                    r.error = Some("cancelled before start".into());
                    return r;
                }
                let dest = job.dest.clone();
                let r = process_file(&settings, &job.input, &|_, _| dest.clone(), threads, &cancel);
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                if !settings.quiet {
                    let _g = print_lock.lock().unwrap_or_else(|p| p.into_inner());
                    println!("{}", progress_line(n, total, &r));
                }
                r
            })
            .collect()
    });

    let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    let summary = Summary::compute(&results, wall_ms);
    let info = RunInfo {
        application: APP_NAME.into(),
        version: APP_VERSION.into(),
        started_unix,
        wall_ms,
        jobs: plan.jobs,
        threads: plan.threads,
        logical_cpus: plan.cpus,
        oversubscribed: plan.oversubscribed,
        codecs: codec_versions(),
        settings: settings.clone(),
    };

    let mut report_paths = None;
    if settings.report && !settings.dry_run {
        let report_root = if single_file_output {
            settings
                .output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
        } else {
            settings.output.as_path()
        };
        match reporting::write_reports(report_root, &results, &info, &summary) {
            Ok(p) => report_paths = Some(p),
            Err(e) => eprintln!("warning: could not write reports: {e}"),
        }
    }
    if !settings.quiet && !single_file_output {
        print_summary(&settings, &summary, wall_ms, ignored, report_paths.as_ref().map(|p| (&p.0, &p.1)));
    }

    if cancel.load(Ordering::Relaxed) {
        return Ok(EXIT_CANCELLED);
    }
    Ok(summary.exit_code())
}

fn process_cancelled(settings: &Settings, input: &InputFile) -> FileResult {
    // Reuses the pipeline's result shape without touching the file.
    let cancel = AtomicBool::new(true);
    process_file(settings, input, &|_, _| PathBuf::new(), 1, &cancel)
}

fn progress_line(n: usize, total: usize, r: &FileResult) -> String {
    let name = r.source.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let tag = match r.status {
        Status::Written => "OK  ",
        Status::KeptOriginal => "KEEP",
        Status::DryRun => "DRY ",
        Status::Skipped => "SKIP",
        Status::Failed => "FAIL",
    };
    let w = total.to_string().len();
    let head = format!("[{n:>w$}/{total}] {tag} {name}");
    match r.status {
        Status::Written | Status::KeptOriginal => {
            let pct = if r.source_bytes > 0 {
                (r.output_bytes as f64 - r.source_bytes as f64) / r.source_bytes as f64 * 100.0
            } else {
                0.0
            };
            format!(
                "{head}  {} -> {}  {} -> {}  ({pct:+.1}%)  [{}]  {:.0} ms",
                r.source_format.unwrap_or("?"),
                r.target_format.unwrap_or("?"),
                human_bytes(r.source_bytes),
                human_bytes(r.output_bytes),
                r.verification.as_str(),
                r.timings.total_ms
            )
        }
        Status::DryRun => format!("{head}  {}", r.codec_settings),
        Status::Skipped | Status::Failed => format!("{head}  {}", r.error.as_deref().unwrap_or("")),
    }
}

fn print_summary(settings: &Settings, s: &Summary, wall_ms: f64, ignored: usize, reports: Option<(&PathBuf, &PathBuf)>) {
    println!();
    println!("==================== Summary ====================");
    println!("Files processed : {}", s.files_seen);
    println!(
        "  written {} | kept original {} | dry-run {} | skipped {} | failed {} | ignored non-images {}",
        s.written, s.kept_original, s.dry_run, s.skipped, s.failed, ignored
    );
    if !settings.dry_run {
        println!("Original size   : {}", human_bytes(s.source_bytes));
        println!("Final size      : {}", human_bytes(s.output_bytes));
        println!("Saved           : {} ({:.2}%)", human_bytes(s.saved_bytes.unsigned_abs()), s.saved_percent);
        if s.saved_bytes < 0 {
            println!("                  (negative: format conversions produced larger files)");
        }
        for (k, g) in &s.by_conversion {
            println!(
                "  {k:<18} {:>5} files  {} -> {}",
                g.files,
                human_bytes(g.source_bytes),
                human_bytes(g.output_bytes)
            );
        }
        println!("Throughput      : {:.2} MiB/s (wall clock, input bytes)", s.throughput_mib_s);
    }
    println!(
        "Stage time (sum over files): load {:.0} ms | decode {:.0} ms | encode {:.0} ms | verify {:.0} ms | write {:.0} ms",
        s.load_ms, s.decode_ms, s.encode_ms, s.verify_ms, s.write_ms
    );
    println!("Elapsed         : {} ({:.1} s)", hms(std::time::Duration::from_secs_f64(wall_ms / 1000.0)), wall_ms / 1000.0);
    if let Some((c, j)) = reports {
        println!("Reports         : {}\n                  {}", c.display(), j.display());
    }
    println!("=================================================");
}
