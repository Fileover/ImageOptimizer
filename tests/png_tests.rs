mod common;
use common::*;
use clap::Parser;
use image_optimizer::cli::Cli;
use image_optimizer::config::Settings;
use image_optimizer::engines::oxipng;
use image_optimizer::formats::{png, Verification};
use image_optimizer::image::color::Samples;
use image_optimizer::image::decoder::{decode, DecodeLimits};
use image_optimizer::image::encoder::write_png;
use image_optimizer::image::identification::{apng_info, png_chunks, Format};

fn s_fast() -> Settings {
    settings(&["--no-zopfli", "--png-preset", "2"])
}

fn lim() -> DecodeLimits {
    DecodeLimits { max_pixels: 1 << 26, threads: 1 }
}

#[test]
fn optimization_is_pixel_exact_for_all_layouts_and_depths() {
    let s = s_fast();
    let c = ctx(&s);
    for (ch, depth) in [(1, 8), (2, 8), (3, 8), (4, 8), (3, 16), (4, 16), (1, 16)] {
        let mut r = pattern(40, 30, ch, depth);
        if let Samples::U16(v) = &mut r.samples {
            // Make the samples genuinely 16-bit (high byte != low byte) so no lossless reduction applies.
            for (i, x) in v.iter_mut().enumerate() {
                *x = x.wrapping_add((i % 13) as u16 + 1);
            }
        }
        let src = write_png(&r, &Default::default()).unwrap();
        let enc = png::optimize(&c, &src).unwrap();
        assert_eq!(png::verify_same(&c, &src, &enc.bytes).unwrap(), Verification::PixelIdentical, "ch={ch} depth={depth}");
        let d = decode(&enc.bytes, Format::Png, &lim()).unwrap();
        assert_eq!(d.raster.depth(), depth, "bit depth must not be reduced");
    }
}

#[test]
fn hidden_rgb_under_alpha_zero_is_untouched_by_default_and_optimized_with_flag() {
    let mut r = pattern(32, 32, 4, 8);
    if let Samples::U8(v) = &mut r.samples {
        for px in v.chunks_mut(4).step_by(2) {
            px[3] = 0;
        }
    }
    let src = write_png(&r, &Default::default()).unwrap();

    // Default preserves hidden RGB (upstream oxipng default).
    let s = s_fast();
    let enc = png::optimize(&ctx(&s), &src).unwrap();
    let d = decode(&enc.bytes, Format::Png, &lim()).unwrap();
    assert_eq!(rgba16(&d.raster), rgba16(&r));
    assert_eq!(png::verify_same(&ctx(&s), &src, &enc.bytes).unwrap(), Verification::PixelIdentical);

    // --optimize-alpha optimizes transparent RGB and verifies visible pixels + alpha.
    let s_opt = settings(&["--no-zopfli", "--png-preset", "2", "--optimize-alpha"]);
    let enc_opt = png::optimize(&ctx(&s_opt), &src).unwrap();
    assert!(enc_opt.settings.contains("optimize_alpha=true"));
    assert_eq!(
        png::verify_same(&ctx(&s_opt), &src, &enc_opt.bytes).unwrap(),
        Verification::VisiblePixelsIdentical
    );

    // --optimize-alpha is rejected when --lossless is explicitly requested.
    let cli = Cli::parse_from(["ImageOptimizer", "-i", "in", "-o", "out", "--lossless", "--optimize-alpha"]);
    assert!(Settings::resolve(&cli).is_err());
}

#[test]
fn zopfli_is_used_competitive_and_stays_exact() {
    // A photographic-like image (decoded fixture) instead of white noise.
    let r = image_optimizer::engines::mozjpeg::decode(&fixture("baseline_plain.jpg"), 1 << 26).unwrap().raster;
    let src = write_png(&r, &Default::default()).unwrap();
    let fast = settings(&["--no-zopfli", "--png-preset", "2"]);
    let zop = settings(&["--zopfli-iterations", "15", "--png-preset", "2"]);
    let a = png::optimize(&ctx(&fast), &src).unwrap();
    let b = png::optimize(&ctx(&zop), &src).unwrap();
    // Zopfli is not guaranteed to win on every tiny noisy image; it must not be meaningfully worse.
    assert!(b.bytes.len() as f64 <= a.bytes.len() as f64 * 1.02, "zopfli {} vs libdeflate {}", b.bytes.len(), a.bytes.len());
    assert!(b.settings.contains("zopfli"));
    assert_eq!(png::verify_same(&ctx(&zop), &src, &b.bytes).unwrap(), Verification::PixelIdentical);
}

#[test]
fn metadata_is_preserved_and_strip_options_work() {
    let r = pattern(24, 24, 3, 8);
    let mut m = metadata(6);
    m.icc = Some(fixture_icc());
    let src = write_png(&r, &m).unwrap();
    assert!(png_chunks(&src).iter().any(|(t, _)| t == b"iCCP"));

    let keep = s_fast();
    let enc = png::optimize(&ctx(&keep), &src).unwrap();
    let kinds: Vec<_> = png_chunks(&enc.bytes).into_iter().map(|(t, _)| t).collect();
    assert!(kinds.contains(b"iCCP") && kinds.contains(b"eXIf"), "ICC and EXIF kept: {kinds:?}");
    assert_eq!(png::verify_same(&ctx(&keep), &src, &enc.bytes).unwrap(), Verification::PixelIdentical);
    let d = decode(&enc.bytes, Format::Png, &lim()).unwrap();
    assert!(d.metadata.xmp.is_some(), "XMP (iTXt) kept");

    let strip = settings(&["--no-zopfli", "--strip-metadata"]);
    let enc = png::optimize(&ctx(&strip), &src).unwrap();
    let kinds: Vec<_> = png_chunks(&enc.bytes).into_iter().map(|(t, _)| t).collect();
    assert!(!kinds.contains(b"eXIf") && !kinds.contains(b"iTXt"));
    assert!(kinds.contains(b"iCCP"), "ICC survives --strip-metadata");

    let strip_icc = settings(&["--no-zopfli", "--strip-icc"]);
    let enc = png::optimize(&ctx(&strip_icc), &src).unwrap();
    let kinds: Vec<_> = png_chunks(&enc.bytes).into_iter().map(|(t, _)| t).collect();
    assert!(!kinds.contains(b"iCCP") && kinds.contains(b"eXIf"));
}

fn fixture_icc() -> Vec<u8> {
    let src = fixture("baseline_meta.jpg");
    image_optimizer::engines::mozjpeg::decode(&src, 1 << 26).unwrap().metadata.icc.unwrap()
}

fn make_apng() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = ::png::Encoder::new(&mut out, 8, 8);
        e.set_color(::png::ColorType::Rgba);
        e.set_depth(::png::BitDepth::Eight);
        e.set_animated(3, 0).unwrap();
        let mut w = e.write_header().unwrap();
        for f in 0..3u8 {
            let data: Vec<u8> = (0..8 * 8).flat_map(|i| [f * 80, (i * 3) as u8, 200 - f * 50, 255]).collect();
            w.write_image_data(&data).unwrap();
        }
        w.finish().unwrap();
    }
    out
}

#[test]
fn apng_animation_is_preserved_when_optimizing() {
    let src = make_apng();
    assert_eq!(apng_info(&src).map(|a| a.0), Some(3));
    let s = s_fast();
    let enc = png::optimize(&ctx(&s), &src).unwrap();
    assert_eq!(apng_info(&enc.bytes).map(|a| a.0), Some(3), "all frames must survive");
}

#[test]
fn apng_is_refused_as_conversion_source() {
    let src = make_apng();
    assert!(decode(&src, Format::Png, &lim()).is_err(), "static conversion would drop the animation");
}

#[test]
fn corrupt_png_is_an_error() {
    let s = s_fast();
    let src = write_png(&pattern(16, 16, 3, 8), &Default::default()).unwrap();
    assert!(png::optimize(&ctx(&s), &src[..src.len() - 20]).is_err());
    assert!(png::optimize(&ctx(&s), b"\x89PNG\r\n\x1a\ngarbage").is_err());
}

#[test]
fn engine_version_is_reported() {
    assert!(!oxipng::version().is_empty());
}
