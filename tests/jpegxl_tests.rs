mod common;
use common::*;
use clap::Parser;
use image_optimizer::cli::Cli;
use image_optimizer::config::Settings;
use image_optimizer::engines::libjxl::{self, JxlOptions};
use image_optimizer::formats::{jpegxl, Verification};
use image_optimizer::image::color::Samples;

fn lossless_opts() -> JxlOptions {
    JxlOptions { lossless: true, distance: 0.0, effort: 3, brotli_effort: 9, threads: 1, store_metadata: true }
}

#[test]
fn lossless_roundtrip_is_pixel_exact_for_all_layouts() {
    for (ch, depth) in [(1, 8), (2, 8), (3, 8), (4, 8), (3, 16), (4, 16), (1, 16)] {
        let r = pattern(37, 29, ch, depth);
        let bytes = libjxl::encode(&r, &Default::default(), &lossless_opts()).unwrap();
        let d = libjxl::decode(&bytes, 1 << 26, 1, true).unwrap();
        assert_eq!((d.raster.width, d.raster.height), (37, 29), "ch={ch} depth={depth}");
        assert_eq!(rgba16(&d.raster), rgba16(&r), "ch={ch} depth={depth}");
    }
}

#[test]
fn hidden_rgb_under_transparent_pixels_survives_lossless() {
    let mut r = pattern(16, 16, 4, 8);
    if let Samples::U8(v) = &mut r.samples {
        for px in v.chunks_mut(4).step_by(3) {
            px[3] = 0; // fully transparent, RGB must still be preserved
        }
    }
    let bytes = libjxl::encode(&r, &Default::default(), &lossless_opts()).unwrap();
    let d = libjxl::decode(&bytes, 1 << 26, 1, true).unwrap();
    assert_eq!(rgba16(&d.raster), rgba16(&r));
}

#[test]
fn metadata_and_orientation_are_stored() {
    let r = pattern(20, 10, 3, 8);
    let meta = metadata(6);
    let bytes = libjxl::encode(&r, &meta, &lossless_opts()).unwrap();
    // Stored pixels are untouched (orientation is metadata, not a rotation of the data).
    let d = libjxl::decode(&bytes, 1 << 26, 1, true).unwrap();
    assert_eq!((d.raster.width, d.raster.height), (20, 10));
    assert_eq!(rgba16(&d.raster), rgba16(&r));
    assert!(d.metadata.xmp.as_deref().map(|x| x.windows(8).any(|w| w == b"UnitTest")).unwrap_or(false), "XMP box");
    assert!(d.metadata.exif.is_some(), "Exif box");
    // Displayed decode applies the orientation (6 = rotate 90 degrees): dimensions swap.
    let shown = libjxl::decode(&bytes, 1 << 26, 1, false).unwrap();
    assert_eq!((shown.raster.width, shown.raster.height), (10, 20));
}

#[test]
fn jpeg_transcode_is_default_for_jpeg_and_respects_lossless_jpeg_flag() {
    let s = settings(&[]);
    let c = ctx(&s);
    assert!(jpegxl::should_transcode_jpeg(&c), "cjxl defaults to lossless JPEG transcoding");
    assert!(!jpegxl::should_transcode_jpeg(&ctx(&settings(&["--lossy"]))));
    assert!(!jpegxl::should_transcode_jpeg(&ctx(&settings(&["--quality", "85"]))));
    assert!(!jpegxl::should_transcode_jpeg(&ctx(&settings(&["--distance", "1.5"]))));
    assert!(!jpegxl::should_transcode_jpeg(&ctx(&settings(&["--lossless_jpeg=0"]))));
    assert!(jpegxl::should_transcode_jpeg(&ctx(&settings(&["--distance", "0"]))));

    for name in ["baseline_meta.jpg", "baseline_plain.jpg", "progressive.jpg"] {
        let jpeg = fixture(name);
        let enc = jpegxl::transcode_jpeg(&c, &jpeg).unwrap();
        assert!(enc.lossless);
        assert!(enc.bytes.len() < jpeg.len(), "{name}: JXL should be smaller than the JPEG");
        let v = jpegxl::verify_jpeg_reconstruction(&c, &jpeg, &enc.bytes).unwrap();
        assert_eq!(v, Verification::BitstreamIdentical);
        let rebuilt = libjxl::decode_jpeg(&enc.bytes, 1).unwrap().expect("reconstruction data present");
        assert_eq!(rebuilt, jpeg, "{name}");
    }
}

#[test]
fn cjxl_defaults_expert_options_and_max_compression_match_upstream() {
    // Upstream cjxl default: distance=1.0, effort=7, brotli_effort=9.
    let def = settings(&[]);
    let o_def = jpegxl::options(&ctx(&def)).unwrap();
    assert!(!o_def.lossless);
    assert_eq!(o_def.distance, 1.0);
    assert_eq!(o_def.effort, 7);
    assert_eq!(o_def.brotli_effort, 9);

    // Lossless flags without --effort keep cjxl default effort=7, brotli_effort=9.
    for extra in [&["--lossless"][..], &["--distance=0"][..], &["-d", "0"][..], &["-q", "100"][..]] {
        let s = settings(extra);
        let o = jpegxl::options(&ctx(&s)).unwrap();
        assert!(o.lossless, "{extra:?} must resolve to lossless JXL");
        assert_eq!(o.distance, 0.0);
        assert_eq!(o.effort, 7);
        assert_eq!(o.brotli_effort, 9);
    }

    // Effort 11 requires --allow_expert_options (matching cjxl).
    let no_expert = Cli::parse_from(["ImageOptimizer", "--distance=0", "--effort=11", "in.png", "out.jxl"]);
    assert!(Settings::resolve(&no_expert).is_err());

    // Exact cjxl CLI flags work verbatim:
    let cjxl_max = settings(&["--distance=0", "--effort=11", "--allow_expert_options", "--brotli_effort=11"]);
    let o_cjxl = jpegxl::options(&ctx(&cjxl_max)).unwrap();
    assert!(o_cjxl.lossless);
    assert_eq!(o_cjxl.effort, 11);
    assert_eq!(o_cjxl.brotli_effort, 11);

    // --lossless --max-compression selects effort=11 + brotli_effort=11 automatically.
    let max_ll = settings(&["--lossless", "--max-compression"]);
    let o_max = jpegxl::options(&ctx(&max_ll)).unwrap();
    assert!(o_max.lossless);
    assert_eq!(o_max.effort, 11);
    assert_eq!(o_max.brotli_effort, 11);

    // Effort 11 + brotli_effort 11 actually encodes and roundtrips pixel-exact.
    let small = pattern(24, 24, 4, 8);
    let encoded = libjxl::encode(&small, &metadata(1), &o_cjxl).unwrap();
    let decoded = libjxl::decode(&encoded, 1 << 26, 1, true).unwrap();
    assert_eq!(rgba16(&decoded.raster), rgba16(&small));

    // Explicit --lossy rejects quality 100 / distance 0 so it never silently turns lossless.
    let s_bad = settings(&["--lossy", "--quality", "100"]);
    assert!(jpegxl::options(&ctx(&s_bad)).is_err());
}

#[test]
fn pixel_jxl_has_no_jpeg_reconstruction_data() {
    let bytes = libjxl::encode(&pattern(8, 8, 3, 8), &Default::default(), &lossless_opts()).unwrap();
    assert!(libjxl::decode_jpeg(&bytes, 1).unwrap().is_none());
}

#[test]
fn lossy_is_smaller_and_decodable() {
    let r = pattern(64, 64, 3, 8);
    let lossless = libjxl::encode(&r, &Default::default(), &lossless_opts()).unwrap();
    let lossy_opts = JxlOptions { lossless: false, distance: 2.0, effort: 3, brotli_effort: 9, threads: 1, store_metadata: true };
    let lossy = libjxl::encode(&r, &Default::default(), &lossy_opts).unwrap();
    assert!(lossy.len() < lossless.len());
    let d = libjxl::decode(&lossy, 1 << 26, 1, true).unwrap();
    assert_eq!((d.raster.width, d.raster.height), (64, 64));

    let s_def = settings(&["-e", "3"]);
    let enc = jpegxl::encode(&ctx(&s_def), &r, &Default::default()).unwrap();
    assert!(!enc.lossless);
    assert_eq!(
        jpegxl::verify_pixels(&ctx(&s_def), &r, &enc.bytes, enc.lossless).unwrap(),
        Verification::DecodableOnly
    );
}

#[test]
fn quality_to_distance_mapping_is_monotonic() {
    assert!(libjxl::distance_from_quality(90.0) < libjxl::distance_from_quality(70.0));
    assert!(libjxl::distance_from_quality(100.0) == 0.0);
}

#[test]
fn corrupt_and_truncated_input_return_errors() {
    let bytes = libjxl::encode(&pattern(32, 32, 3, 8), &Default::default(), &lossless_opts()).unwrap();
    assert!(libjxl::decode(&bytes[..bytes.len() / 2], 1 << 26, 1, true).is_err());
    assert!(libjxl::decode(b"\xFF\x0Anot really jxl", 1 << 26, 1, true).is_err());
}

#[test]
fn pixel_limit_is_enforced() {
    let bytes = libjxl::encode(&pattern(64, 64, 3, 8), &Default::default(), &lossless_opts()).unwrap();
    assert!(libjxl::decode(&bytes, 100, 1, true).is_err());
}
