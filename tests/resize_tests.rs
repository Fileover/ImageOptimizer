//! End-to-end and raster tests for downscale-only `--resize`.
mod common;
use common::*;
use image_optimizer::engines::{libjxl, libwebp, mozjpeg};
use image_optimizer::image::color::{Raster, Samples};
use image_optimizer::image::decoder::{decode, DecodeLimits};
use image_optimizer::image::encoder::write_png;
use image_optimizer::image::identification::{identify, Format};
use image_optimizer::image::resize::{resize_raster, ResizeFilter};
use std::process::{Command, Output};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ImageOptimizer"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().expect("executable runs")
}

fn dims(bytes: &[u8]) -> (u32, u32) {
    let fmt = identify(bytes).expect("valid image");
    let d = decode(bytes, fmt, &DecodeLimits { max_pixels: 1 << 26, threads: 1 }).expect("decodable");
    (d.raster.width, d.raster.height)
}

#[test]
fn downscales_512x512_to_256x256_across_all_output_formats() {
    let tmp = tempfile::tempdir().unwrap();
    let src_png = tmp.path().join("src512.png");
    std::fs::write(&src_png, write_png(&pattern(512, 512, 3, 8), &Default::default()).unwrap()).unwrap();

    for (name, extra) in [
        ("out.png", &["--no-zopfli", "--png-preset", "1"][..]),
        ("out.webp", &[][..]),
        ("out_ll.webp", &["--lossless"][..]),
        ("out.jpg", &[][..]),
        ("out.jxl", &["--effort", "3"][..]),
        ("out_ll.jxl", &["--lossless", "--effort", "3"][..]),
    ] {
        let out = tmp.path().join(name);
        let mut args = vec![src_png.to_str().unwrap(), out.to_str().unwrap(), "--resize", "256x256"];
        args.extend_from_slice(extra);
        let o = run(&args);
        assert_eq!(o.status.code(), Some(0), "{name}: {}", String::from_utf8_lossy(&o.stderr));
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(dims(&bytes), (256, 256), "{name}");
    }
    assert!(!tmp.path().join("Reports").exists());
}

#[test]
fn preserves_aspect_ratio_and_supports_single_axis_and_exact_downscale() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("wide.png");
    std::fs::write(&src, write_png(&pattern(400, 200, 3, 8), &Default::default()).unwrap()).unwrap();

    let fit = tmp.path().join("fit.webp");
    assert_eq!(run(&[src.to_str().unwrap(), fit.to_str().unwrap(), "--resize", "200x200"]).status.code(), Some(0));
    assert_eq!(dims(&std::fs::read(&fit).unwrap()), (200, 100));

    let w_only = tmp.path().join("w.webp");
    assert_eq!(run(&[src.to_str().unwrap(), w_only.to_str().unwrap(), "--resize", "100x"]).status.code(), Some(0));
    assert_eq!(dims(&std::fs::read(&w_only).unwrap()), (100, 50));

    let h_only = tmp.path().join("h.webp");
    assert_eq!(run(&[src.to_str().unwrap(), h_only.to_str().unwrap(), "--resize", "x100"]).status.code(), Some(0));
    assert_eq!(dims(&std::fs::read(&h_only).unwrap()), (200, 100));

    let exact = tmp.path().join("exact.webp");
    assert_eq!(run(&[src.to_str().unwrap(), exact.to_str().unwrap(), "--resize", "200x150!"]).status.code(), Some(0));
    assert_eq!(dims(&std::fs::read(&exact).unwrap()), (200, 150));

    // Exact mode still never upscales an axis that is already smaller (height 200 <= 300).
    let exact_clamp = tmp.path().join("exact_clamp.webp");
    assert_eq!(run(&[src.to_str().unwrap(), exact_clamp.to_str().unwrap(), "--resize", "200x300!"]).status.code(), Some(0));
    assert_eq!(dims(&std::fs::read(&exact_clamp).unwrap()), (200, 200));
}

#[test]
fn never_upscales_and_keeps_lossless_fast_paths_when_already_within_bounds() {
    let tmp = tempfile::tempdir().unwrap();
    let small_png = tmp.path().join("small.png");
    std::fs::write(&small_png, write_png(&pattern(64, 48, 3, 8), &Default::default()).unwrap()).unwrap();

    // Small PNG stays 64x48 even when --resize 256x256 is requested.
    let out_png = tmp.path().join("out.png");
    assert_eq!(
        run(&[small_png.to_str().unwrap(), out_png.to_str().unwrap(), "--resize", "256x256", "--no-zopfli"]).status.code(),
        Some(0)
    );
    assert_eq!(dims(&std::fs::read(&out_png).unwrap()), (64, 48));

    // Small JPEG -> JXL with larger --resize stays on the lossless JPEG transcode fast path.
    let small_jpg = tmp.path().join("small.jpg");
    let jpg_bytes = fixture("baseline_plain.jpg");
    let (jw, jh) = dims(&jpg_bytes);
    std::fs::write(&small_jpg, &jpg_bytes).unwrap();

    let out_jxl = tmp.path().join("small.jxl");
    let big_spec = format!("{}x{}", jw * 2, jh * 2);
    assert_eq!(
        run(&[small_jpg.to_str().unwrap(), out_jxl.to_str().unwrap(), "--resize", &big_spec, "--effort", "3"]).status.code(),
        Some(0)
    );
    let rebuilt = libjxl::decode_jpeg(&std::fs::read(&out_jxl).unwrap(), 1).unwrap();
    assert_eq!(rebuilt, Some(jpg_bytes), "already-small JPEG must keep lossless JXL transcode");
}

#[test]
fn jpeg_downscale_reencodes_with_mozjpeg_by_default_and_refuses_lossless() {
    let tmp = tempfile::tempdir().unwrap();
    let src_jpg = tmp.path().join("src.jpg");
    let jpg_bytes = fixture("baseline_plain.jpg");
    let (jw, jh) = dims(&jpg_bytes);
    assert!(jw > 32 && jh > 32);
    std::fs::write(&src_jpg, &jpg_bytes).unwrap();

    // Default mode: downscales and re-encodes via MozJPEG.
    let out_jpg = tmp.path().join("small.jpg");
    assert_eq!(
        run(&[src_jpg.to_str().unwrap(), out_jpg.to_str().unwrap(), "--resize", "32x32"]).status.code(),
        Some(0)
    );
    let (ow, oh) = dims(&std::fs::read(&out_jpg).unwrap());
    assert!(ow <= 32 && oh <= 32 && (ow == 32 || oh == 32));

    // --lossless refuses resizing JPEG -> JPEG because re-encoding JPEG is lossy.
    let out_dir = tmp.path().join("out_ll");
    let o = run(&[
        "--input",
        src_jpg.to_str().unwrap(),
        "--output",
        out_dir.to_str().unwrap(),
        "--format",
        "jpeg",
        "--lossless",
        "--resize",
        "32x32",
        "--report",
    ]);
    assert_eq!(o.status.code(), Some(0));
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out_dir.join("Reports/report.json")).unwrap()).unwrap();
    assert_eq!(json["files"][0]["status"], "skipped");
    assert!(json["files"][0]["error"].as_str().unwrap().contains("--lossless"));
}

#[test]
fn transparent_rgba_downscale_avoids_dark_halos_and_preserves_16bit_png() {
    // 64x64 RGBA: left half opaque white (255,255,255,255), right half transparent black (0,0,0,0).
    let mut pixels = Vec::with_capacity(64 * 64 * 4);
    for _y in 0..64 {
        for x in 0..64 {
            if x < 32 {
                pixels.extend_from_slice(&[255, 255, 255, 255]);
            } else {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    let r = Raster { width: 64, height: 64, channels: 4, samples: Samples::U8(pixels), is_cmyk: false };
    let resized = resize_raster(&r, 32, 32, ResizeFilter::Triangle).unwrap();
    if let Samples::U8(v) = &resized.samples {
        for px in v.chunks_exact(4) {
            if px[3] > 0 {
                // Every visible or semi-transparent edge pixel must remain pure white (no black fringe).
                assert!(px[0] >= 250 && px[1] >= 250 && px[2] >= 250, "dark halo detected: {px:?}");
            }
        }
    } else {
        panic!("expected 8-bit");
    }

    // 16-bit PNG downscale stays 16-bit.
    let tmp = tempfile::tempdir().unwrap();
    let mut r16 = pattern(64, 64, 3, 16);
    if let Samples::U16(v) = &mut r16.samples {
        for (i, s) in v.iter_mut().enumerate() {
            *s = s.wrapping_add((i % 17) as u16 + 1);
        }
    }
    let src16 = tmp.path().join("src16.png");
    let out16 = tmp.path().join("out16.png");
    std::fs::write(&src16, write_png(&r16, &Default::default()).unwrap()).unwrap();
    assert_eq!(
        run(&[src16.to_str().unwrap(), out16.to_str().unwrap(), "--resize", "32x32", "--no-zopfli", "--png-preset", "1"]).status.code(),
        Some(0)
    );
    let d = decode(&std::fs::read(&out16).unwrap(), Format::Png, &DecodeLimits { max_pixels: 1 << 26, threads: 1 }).unwrap();
    assert_eq!((d.raster.width, d.raster.height, d.raster.depth()), (32, 32, 16));
}

#[test]
fn invalid_resize_spec_returns_usage_error_and_config_resize_works() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("a.png");
    let dst = tmp.path().join("out.webp");
    std::fs::write(&src, write_png(&pattern(64, 64, 3, 8), &Default::default()).unwrap()).unwrap();

    assert_eq!(run(&[src.to_str().unwrap(), dst.to_str().unwrap(), "--resize", "0x0"]).status.code(), Some(2));
    assert_eq!(run(&[src.to_str().unwrap(), dst.to_str().unwrap(), "--resize", "bad"]).status.code(), Some(2));
    assert_eq!(run(&[src.to_str().unwrap(), dst.to_str().unwrap(), "--resize", "32x!"]).status.code(), Some(2));

    let cfg = tmp.path().join("cfg.json");
    std::fs::write(
        &cfg,
        format!(r#"{{"input":{:?},"output":{:?},"resize":"32x32","resize_filter":"catmull-rom"}}"#, src, dst),
    )
    .unwrap();
    assert_eq!(run(&["--config", cfg.to_str().unwrap()]).status.code(), Some(0));
    let d = libwebp::decode(&std::fs::read(&dst).unwrap(), 1 << 26).unwrap();
    assert_eq!((d.raster.width, d.raster.height), (32, 32));
    // Verify MozJPEG decode also handles resized JPEG cleanly.
    let dst_jpg = tmp.path().join("out.jpg");
    assert_eq!(run(&[src.to_str().unwrap(), dst_jpg.to_str().unwrap(), "--resize", "32x32"]).status.code(), Some(0));
    let dj = mozjpeg::decode(&std::fs::read(&dst_jpg).unwrap(), 1 << 26).unwrap();
    assert_eq!((dj.raster.width, dj.raster.height), (32, 32));
}
