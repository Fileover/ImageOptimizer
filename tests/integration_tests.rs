//! End-to-end tests of the executable (it is run as a test subject only; the product itself
//! never spawns external processes).
mod common;
use common::*;
use image_optimizer::engines::{libjxl, libwebp};
use image_optimizer::image::encoder::write_png;
use image_optimizer::image::identification::{identify, webp_is_lossless, Format};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ImageOptimizer"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().expect("executable runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut m = BTreeMap::new();
    for e in walkdir::WalkDir::new(dir).into_iter().flatten() {
        if e.file_type().is_file() {
            m.insert(e.path().to_path_buf(), std::fs::read(e.path()).unwrap());
        }
    }
    m
}

struct Fixture {
    _tmp: tempfile::TempDir,
    input: PathBuf,
    output: PathBuf,
}

fn fixture_dir() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("in");
    let output = tmp.path().join("out");
    std::fs::create_dir_all(input.join("sub")).unwrap();
    let raster = pattern(48, 32, 3, 8);
    let png = write_png(&raster, &metadata(1)).unwrap();
    std::fs::write(input.join("a.png"), &png).unwrap();
    std::fs::write(input.join("b.jpg"), fixture("baseline_plain.jpg")).unwrap();
    std::fs::write(input.join("sub").join("c.jpeg"), fixture("progressive.jpg")).unwrap();
    let webp = libwebp::encode(
        &raster,
        &Default::default(),
        &libwebp::WebpOptions {
            lossless: true,
            lossless_preset: None,
            quality: Some(100.0),
            method: Some(2),
            alpha_quality: 100,
            threads: 1,
            exact: true,
            sharp_yuv: false,
        },
    )
    .unwrap();
    std::fs::write(input.join("d.webp"), webp).unwrap();
    let jxl = libjxl::encode(
        &raster,
        &Default::default(),
        &libjxl::JxlOptions { lossless: true, distance: 0.0, effort: 3, brotli_effort: 9, threads: 1, store_metadata: true },
    )
    .unwrap();
    std::fs::write(input.join("e.jxl"), jxl).unwrap();
    // Magic bytes win over the extension: a PNG named .jpg.
    std::fs::write(input.join("disguised.jpg"), write_png(&pattern(16, 16, 3, 8), &Default::default()).unwrap()).unwrap();
    std::fs::write(input.join("readme.txt"), "not an image").unwrap();
    Fixture { _tmp: tmp, input, output }
}

fn base<'a>(f: &'a Fixture, extra: &[&'a str]) -> Vec<&'a str> {
    let mut a = vec!["--input", f.input.to_str().unwrap(), "--output", f.output.to_str().unwrap(), "--quiet"];
    a.extend_from_slice(extra);
    a
}

#[test]
fn single_file_positional_infers_format_and_creates_no_reports() {
    let f = fixture_dir();
    let src_png = f.input.join("a.png");
    let src_jpg = f.input.join("b.jpg");
    let work = f.output.clone();
    std::fs::create_dir_all(&work).unwrap();

    // PNG -> WebP (positional, inferred format)
    let out_webp = work.join("photo.webp");
    let o = run(&[src_png.to_str().unwrap(), out_webp.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    assert!(out_webp.is_file());
    let webp_bytes = std::fs::read(&out_webp).unwrap();
    assert_eq!(identify(&webp_bytes), Some(Format::WebP));
    assert_eq!(webp_is_lossless(&webp_bytes), Some(false));
    assert!(!work.join("Reports").exists(), "single-file mode must not create Reports/");
    assert!(!work.join("WebP").exists(), "single-file mode must not create format subfolder");

    // PNG -> JXL lossless into nested subdir (creates parent dir automatically)
    let out_jxl = work.join("nested").join("photo.jxl");
    let o = run(&[src_png.to_str().unwrap(), out_jxl.to_str().unwrap(), "--lossless", "--effort", "3"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    assert_eq!(identify(&std::fs::read(&out_jxl).unwrap()), Some(Format::JpegXl));
    assert!(!work.join("nested").join("Reports").exists());

    // JPEG -> JXL (lossless JPEG transcode by default)
    let out_jxl_jpg = work.join("from_jpg.jxl");
    let o = run(&[src_jpg.to_str().unwrap(), out_jxl_jpg.to_str().unwrap(), "--effort", "3"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    let rebuilt = libjxl::decode_jpeg(&std::fs::read(&out_jxl_jpg).unwrap(), 1).unwrap();
    assert_eq!(rebuilt, Some(std::fs::read(&src_jpg).unwrap()));

    // Exact cjxl-style options-before-paths invocation:
    // ImageOptimizer --distance=0 --effort=11 --allow_expert_options --brotli_effort=11 input output
    let out_cjxl = work.join("cjxl_style.jxl");
    let o = run(&[
        "--distance=0",
        "--effort=11",
        "--allow_expert_options",
        "--brotli_effort=11",
        src_png.to_str().unwrap(),
        out_cjxl.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    assert_eq!(identify(&std::fs::read(&out_cjxl).unwrap()), Some(Format::JpegXl));

    // PNG -> JPEG and PNG -> PNG (options before paths)
    let out_jpg = work.join("photo.jpg");
    assert_eq!(run(&[src_png.to_str().unwrap(), out_jpg.to_str().unwrap()]).status.code(), Some(0));
    assert_eq!(identify(&std::fs::read(&out_jpg).unwrap()), Some(Format::Jpeg));

    let out_png = work.join("photo.png");
    assert_eq!(
        run(&["--opt", "1", src_png.to_str().unwrap(), out_png.to_str().unwrap()]).status.code(),
        Some(0)
    );
    assert_eq!(identify(&std::fs::read(&out_png).unwrap()), Some(Format::Png));
    assert!(!work.join("Reports").exists());
}

#[test]
fn single_file_flags_and_relative_cwd_paths_work() {
    let f = fixture_dir();
    let o = bin()
        .current_dir(&f.input)
        .args(["-i", "a.png", "-o", "converted.webp"])
        .output()
        .expect("executable runs");
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    assert!(f.input.join("converted.webp").is_file());
    assert!(!f.input.join("Reports").exists());
}

#[test]
fn single_file_edge_cases_unknown_extension_overwrite_and_same_source() {
    let f = fixture_dir();
    let src = f.input.join("a.png");
    let orig_bytes = std::fs::read(&src).unwrap();

    // Unknown output extension without --format is rejected (exit code 2).
    let bad = f.output.join("photo.xyz");
    assert_eq!(run(&[src.to_str().unwrap(), bad.to_str().unwrap()]).status.code(), Some(2));
    assert!(!bad.exists());

    // Output path equal to source path is refused (exit code 1) and source stays untouched.
    assert_eq!(run(&[src.to_str().unwrap(), src.to_str().unwrap()]).status.code(), Some(1));
    assert_eq!(std::fs::read(&src).unwrap(), orig_bytes);

    // Existing output file is not overwritten without --overwrite.
    std::fs::create_dir_all(&f.output).unwrap();
    let dst = f.output.join("out.webp");
    std::fs::write(&dst, b"sentinel").unwrap();
    assert_eq!(run(&[src.to_str().unwrap(), dst.to_str().unwrap()]).status.code(), Some(0));
    assert_eq!(std::fs::read(&dst).unwrap(), b"sentinel");
    assert_eq!(run(&[src.to_str().unwrap(), dst.to_str().unwrap(), "--overwrite"]).status.code(), Some(0));
    assert_eq!(identify(&std::fs::read(&dst).unwrap()), Some(Format::WebP));
}

#[test]
fn every_target_format_works_in_default_and_lossless_modes_without_modifying_sources() {
    for fmt in ["jxl", "webp", "jpeg", "png", "original"] {
        let f = fixture_dir();
        let before = snapshot(&f.input);
        let o = run(&base(&f, &["--format", fmt, "--recursive", "--no-zopfli", "--png-preset", "1", "--effort", "3", "--report"]));
        assert_eq!(o.status.code(), Some(0), "{fmt}: {}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
        assert_eq!(before, snapshot(&f.input), "{fmt}: source files must be untouched");
        assert!(f.output.join("Reports/report.csv").exists());
        assert!(f.output.join("Reports/report.json").exists());
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(f.output.join("Reports/report.json")).unwrap()).unwrap();
        assert_eq!(json["summary"]["failed"], 0, "{fmt}");
        let n = json["files"].as_array().unwrap().len();
        assert_eq!(n, 6, "{fmt}: six images (the .txt is ignored)");
        for file in json["files"].as_array().unwrap() {
            if file["status"] == "written" || file["status"] == "kept-original" {
                assert!(Path::new(file["output"].as_str().unwrap()).exists());
                assert_ne!(file["verification"], "not-run", "{fmt}: verification is on by default");
            }
        }
    }
}

#[test]
fn default_mode_follows_upstream_codec_defaults() {
    // 1. --format webp -> cwebp default (lossy VP8, q=75), no Reports/ by default
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "webp"])).status.code(), Some(0));
    let out_webp = std::fs::read(f.output.join("WebP/a.webp")).unwrap();
    assert_eq!(webp_is_lossless(&out_webp), Some(false), "cwebp default is lossy");
    assert!(!f.output.join("Reports").exists(), "Reports/ must not be created without --report");

    // 2. --format jxl -> cjxl default: PNG is lossy (d=1.0), JPEG is lossless transcode
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "jxl", "--effort", "3", "--report"])).status.code(), Some(0));
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.output.join("Reports/report.json")).unwrap()).unwrap();
    let files = json["files"].as_array().unwrap();
    let a = files.iter().find(|x| x["source"].as_str().unwrap().ends_with("a.png")).unwrap();
    let b = files.iter().find(|x| x["source"].as_str().unwrap().ends_with("b.jpg")).unwrap();
    assert_eq!(a["mode"], "lossy", "PNG -> JXL defaults to cjxl visually lossless (d=1.0)");
    assert_eq!(a["verification"], "decodable-lossy-not-compared");
    assert_eq!(b["mode"], "lossless", "JPEG -> JXL defaults to cjxl lossless JPEG transcode");
    assert_eq!(b["verification"], "jpeg-bitstream-identical");

    // 3. --format jpeg -> PNG converts with MozJPEG (no --lossy needed), JPEG optimizes losslessly
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "jpeg", "--report"])).status.code(), Some(0));
    assert!(f.output.join("JPEG/a.jpg").exists(), "PNG -> JPEG works by default");
    assert!(f.output.join("JPEG/b.jpg").exists());
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.output.join("Reports/report.json")).unwrap()).unwrap();
    let files = json["files"].as_array().unwrap();
    let a = files.iter().find(|x| x["source"].as_str().unwrap().ends_with("a.png")).unwrap();
    let b = files.iter().find(|x| x["source"].as_str().unwrap().ends_with("b.jpg")).unwrap();
    assert_eq!(a["mode"], "lossy");
    assert_eq!(b["mode"], "lossless");
    assert_eq!(b["verification"], "dct-coefficients-identical");

    // 4. --format original preserves lossless WebP (VP8L) as lossless
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "original", "--no-zopfli", "--effort", "3"])).status.code(), Some(0));
    let out_d = std::fs::read(f.output.join("WebP/d.webp")).unwrap();
    assert_eq!(webp_is_lossless(&out_d), Some(true), "lossless WebP stays lossless under --format original");
}

#[test]
fn output_layout_has_one_folder_per_format_and_keeps_subdirectories() {
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "jxl", "--recursive", "--effort", "3"])).status.code(), Some(0));
    assert!(f.output.join("JPEG-XL/a.jxl").exists());
    assert!(f.output.join("JPEG-XL/sub/c.jxl").exists());
    assert!(!f.output.join("Reports").exists());
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "original", "--recursive", "--no-zopfli", "--effort", "3"])).status.code(), Some(0));
    assert!(f.output.join("PNG/a.png").exists());
    assert!(f.output.join("JPEG/sub/c.jpeg").exists());
    assert!(f.output.join("WebP/d.webp").exists());
    assert!(f.output.join("JPEG-XL/e.jxl").exists());
    assert!(!f.output.join("Reports").exists());
}

#[test]
fn non_recursive_scan_ignores_subdirectories() {
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "jxl", "--effort", "3"])).status.code(), Some(0));
    assert!(!f.output.join("JPEG-XL/sub").exists());
}

#[test]
fn detection_uses_magic_bytes_not_extensions() {
    let f = fixture_dir();
    let o = run(&[
        "--input",
        f.input.join("disguised.jpg").to_str().unwrap(),
        "--output",
        f.output.to_str().unwrap(),
        "--format",
        "original",
        "--no-zopfli",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    assert!(stdout(&o).contains("PNG -> PNG"));
}

#[test]
fn existing_outputs_are_not_replaced_without_overwrite() {
    let f = fixture_dir();
    let args = base(&f, &["--format", "jxl", "--effort", "3"]);
    assert_eq!(run(&args).status.code(), Some(0));
    let first = snapshot(&f.output.join("JPEG-XL"));
    let marker = f.output.join("JPEG-XL/a.jxl");
    std::fs::write(&marker, b"sentinel").unwrap();
    assert_eq!(run(&args).status.code(), Some(0));
    assert_eq!(std::fs::read(&marker).unwrap(), b"sentinel", "must not be replaced");
    let mut with = args.clone();
    with.push("--overwrite");
    assert_eq!(run(&with).status.code(), Some(0));
    assert_eq!(snapshot(&f.output.join("JPEG-XL")).get(&marker), first.get(&marker), "replaced by a valid file");
}

#[test]
fn dry_run_writes_nothing() {
    let f = fixture_dir();
    let o = run(&["--input", f.input.to_str().unwrap(), "--output", f.output.to_str().unwrap(), "--format", "webp", "--dry-run"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(!f.output.exists(), "dry run must not even create the output folder");
    assert!(stdout(&o).contains("DRY"));
}

#[test]
fn corrupt_file_fails_alone_and_sets_exit_code_1() {
    let f = fixture_dir();
    let good = fixture("baseline_plain.jpg");
    std::fs::write(f.input.join("broken.jpg"), &good[..good.len() / 2]).unwrap();
    let o = run(&base(&f, &["--format", "original", "--no-zopfli", "--effort", "3", "--report"]));
    assert_eq!(o.status.code(), Some(1));
    assert!(f.output.join("PNG/a.png").exists(), "other files are still processed");
    assert!(!f.output.join("JPEG/broken.jpg").exists(), "no partial output for the broken file");
    let csv = std::fs::read_to_string(f.output.join("Reports/report.csv")).unwrap();
    assert!(csv.contains("broken.jpg") && csv.contains("failed"));
    // No temporary files left behind.
    assert!(snapshot(&f.output).keys().all(|p| !p.to_string_lossy().ends_with(".tmp")));
}

#[test]
fn lossless_flag_refuses_lossy_conversions_with_a_clear_reason() {
    let f = fixture_dir();
    let o = run(&base(&f, &["--format", "jpeg", "--lossless", "--no-zopfli", "--report"]));
    assert_eq!(o.status.code(), Some(0));
    assert!(!f.output.join("JPEG/a.jpg").exists(), "PNG -> JPEG is refused under --lossless");
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(f.output.join("Reports/report.json")).unwrap()).unwrap();
    let a = json["files"].as_array().unwrap().iter().find(|x| x["source"].as_str().unwrap().ends_with("a.png")).unwrap();
    assert_eq!(a["status"], "skipped");
    assert!(a["error"].as_str().unwrap().contains("--lossless"));
    assert!(f.output.join("JPEG/b.jpg").exists(), "JPEG -> JPEG is lossless and allowed");
}

#[test]
fn quality_and_distance_match_upstream_rules_and_guards() {
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "webp", "-q", "80"])).status.code(), Some(0));
    assert_eq!(run(&base(&f, &["--format", "jxl", "-d", "1.5", "-e", "3", "--overwrite"])).status.code(), Some(0));
    // cwebp allows -lossless -q 80 (effort trade-off); jpeg/jxl reject lossy quality/distance with --lossless.
    assert_eq!(run(&base(&f, &["--format", "webp", "--lossless", "-q", "80", "--overwrite"])).status.code(), Some(0));
    assert_eq!(run(&base(&f, &["--format", "jpeg", "--lossless", "-q", "80"])).status.code(), Some(2));
    assert_eq!(run(&base(&f, &["--format", "jxl", "--lossless", "-q", "80"])).status.code(), Some(2));
    assert_eq!(run(&base(&f, &["--format", "jxl", "--lossless", "--distance", "1.0"])).status.code(), Some(2));
    assert_eq!(run(&base(&f, &["--format", "jxl", "--effort", "11"])).status.code(), Some(2));
    assert_eq!(run(&base(&f, &["--format", "webp", "--quality", "150"])).status.code(), Some(2));
    assert_eq!(run(&base(&f, &["--format", "jxl", "--lossy", "--distance", "0"])).status.code(), Some(2));
}

#[test]
fn explicit_lossy_forces_lossy_on_jpeg_and_is_reported() {
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "jpeg", "--lossy", "--quality", "55", "--recursive", "--report"])).status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(f.output.join("Reports/report.json")).unwrap()).unwrap();
    assert!(json["files"].as_array().unwrap().iter().all(|x| x["mode"] == "lossy"));
    assert!(json["files"].as_array().unwrap().iter().any(|x| x["verification"] == "decodable-lossy-not-compared"));
}

#[test]
fn larger_results_are_not_written_unless_keep_larger() {
    // A file that is already optimal: re-optimizing cannot make it smaller.
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "original", "--no-zopfli", "--recursive", "--effort", "3"])).status.code(), Some(0));
    let optimized = f.output.join("PNG/a.png");
    let out2 = f.output.parent().unwrap().join("out2");
    let o = run(&[
        "--input",
        optimized.to_str().unwrap(),
        "--output",
        out2.to_str().unwrap(),
        "--format",
        "original",
        "--no-zopfli",
        "--quiet",
        "--report",
    ]);
    assert_eq!(o.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(out2.join("Reports/report.json")).unwrap()).unwrap();
    let r = &json["files"][0];
    if r["status"] == "kept-original" {
        assert_eq!(std::fs::read(out2.join("PNG/a.png")).unwrap(), std::fs::read(&optimized).unwrap());
        assert_eq!(r["verification"], "byte-identical-copy");
    }
}

#[test]
fn jobs_and_threads_are_accepted_and_reported() {
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "webp", "--jobs", "2", "--threads", "1", "--recursive", "--report"])).status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(f.output.join("Reports/report.json")).unwrap()).unwrap();
    assert_eq!(json["run"]["jobs"], 2);
    assert_eq!(json["run"]["threads"], 1);
    assert_eq!(run(&base(&f, &["--jobs", "0"])).status.code(), Some(2));
}

#[test]
fn reports_are_opt_in_and_have_per_stage_timings_and_csv_header() {
    let f = fixture_dir();
    assert_eq!(run(&base(&f, &["--format", "png", "--no-zopfli"])).status.code(), Some(0));
    assert!(!f.output.join("Reports").exists(), "Reports/ must not be created by default");

    assert_eq!(run(&base(&f, &["--format", "png", "--no-zopfli", "--overwrite", "--report"])).status.code(), Some(0));
    let csv = std::fs::read_to_string(f.output.join("Reports/report.csv")).unwrap();
    let header = csv.lines().next().unwrap();
    for col in ["source", "status", "source_bytes", "output_bytes", "saved_percent", "verification", "load_ms", "decode_ms", "encode_ms", "verify_ms", "write_ms", "total_ms"] {
        assert!(header.split(',').any(|c| c == col), "missing column {col}");
    }
}

#[test]
fn json_config_is_merged_and_cli_wins() {
    let f = fixture_dir();
    let cfg = f.input.parent().unwrap().join("cfg.json");
    std::fs::write(&cfg, format!(r#"{{"format":"webp","lossless":true,"recursive":true,"input":{:?},"output":{:?}}}"#, f.input, f.output)).unwrap();
    assert_eq!(run(&["--config", cfg.to_str().unwrap(), "--quiet"]).status.code(), Some(0));
    assert!(f.output.join("WebP/sub/c.webp").exists());
    assert_eq!(webp_is_lossless(&std::fs::read(f.output.join("WebP/a.webp")).unwrap()), Some(true));

    // CLI --lossy overrides config lossless:true.
    assert_eq!(run(&["--config", cfg.to_str().unwrap(), "--lossy", "--overwrite", "--quiet"]).status.code(), Some(0));
    assert_eq!(webp_is_lossless(&std::fs::read(f.output.join("WebP/a.webp")).unwrap()), Some(false));

    std::fs::write(&cfg, r#"{"bogus_key":1}"#).unwrap();
    assert_eq!(run(&["--config", cfg.to_str().unwrap()]).status.code(), Some(2), "unknown keys are rejected");
}

#[test]
fn version_and_format_listing() {
    let v = stdout(&run(&["--version"]));
    for s in ["libjxl", "libwebp", "mozjpeg", "jpegoptim", "oxipng", "GPL-3.0"] {
        assert!(v.contains(s), "--version should mention {s}");
    }
    assert!(stdout(&run(&["--list-formats"])).contains("JPEG XL"));
}

#[test]
fn no_arguments_prints_overview_and_exits_cleanly() {
    let o = run(&[]);
    assert_eq!(o.status.code(), Some(0));
    let out = stdout(&o);
    for needle in ["ImageOptimizer", "libjxl", "libwebp", "mozjpeg", "oxipng", "Usage:", "--help"] {
        assert!(out.contains(needle), "no-arg output should mention {needle}: {out}");
    }
}
