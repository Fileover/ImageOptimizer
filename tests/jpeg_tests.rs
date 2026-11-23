mod common;
use common::*;
use image_optimizer::engines::{jpegoptim, mozjpeg};
use image_optimizer::formats::{jpeg, Verification};

fn default_settings() -> image_optimizer::config::Settings {
    settings(&[])
}

#[test]
fn default_jpeg_optimization_is_lossless_and_shrinks_baseline_files() {
    let s = default_settings();
    let c = ctx(&s);
    for name in ["baseline_plain.jpg", "baseline_meta.jpg", "progressive.jpg"] {
        let src = fixture(name);
        let enc = jpeg::optimize(&c, &src).unwrap();
        assert!(enc.lossless, "{name}: jpegoptim default must be lossless");
        assert_eq!(
            jpeg::verify_optimized(&c, &src, &enc.bytes, enc.lossless).unwrap(),
            Verification::CoefficientsIdentical,
            "{name}"
        );
        // Independent comparison through the engine API.
        assert!(jpegoptim::coefficients_equal(&src, &enc.bytes, true).unwrap());
    }
    let src = fixture("baseline_plain.jpg");
    let enc = jpeg::optimize(&c, &src).unwrap();
    assert!(enc.bytes.len() < src.len(), "Huffman optimization must shrink an unoptimized baseline JPEG");
}

#[test]
fn metadata_and_icc_are_kept_by_default_and_pixels_identical() {
    let s = default_settings();
    let src = fixture("baseline_meta.jpg");
    let enc = jpeg::optimize(&ctx(&s), &src).unwrap();
    let a = mozjpeg::decode(&src, 1 << 26).unwrap();
    let b = mozjpeg::decode(&enc.bytes, 1 << 26).unwrap();
    assert_eq!(rgba16(&a.raster), rgba16(&b.raster));
    assert!(b.metadata.exif.is_some() && b.metadata.xmp.is_some());
    assert_eq!(a.metadata.icc, b.metadata.icc);
    assert_eq!(a.metadata.exif, b.metadata.exif);
}

#[test]
fn strip_metadata_removes_exif_and_xmp_but_keeps_icc() {
    let s = settings(&["--strip-metadata"]);
    let src = fixture("baseline_meta.jpg");
    let enc = jpeg::optimize(&ctx(&s), &src).unwrap();
    let b = mozjpeg::decode(&enc.bytes, 1 << 26).unwrap();
    assert!(b.metadata.exif.is_none());
    assert!(b.metadata.xmp.is_none());
    assert!(b.metadata.icc.is_some(), "ICC survives --strip-metadata");
    assert_eq!(
        jpeg::verify_optimized(&ctx(&s), &src, &enc.bytes, enc.lossless).unwrap(),
        Verification::CoefficientsIdentical
    );
}

#[test]
fn strip_icc_removes_the_profile() {
    let s = settings(&["--strip-icc"]);
    let enc = jpeg::optimize(&ctx(&s), &fixture("baseline_meta.jpg")).unwrap();
    let b = mozjpeg::decode(&enc.bytes, 1 << 26).unwrap();
    assert!(b.metadata.icc.is_none());
    assert!(b.metadata.exif.is_some());
}

#[test]
fn jfif_density_is_preserved() {
    let s = default_settings();
    let src = fixture("baseline_meta.jpg"); // written with 300 dpi
    let enc = jpeg::optimize(&ctx(&s), &src).unwrap();
    let density = |d: &[u8]| -> Option<(u8, u16, u16)> {
        let i = d.windows(5).position(|w| w == b"JFIF\0")?;
        let p = &d[i + 5 + 2..]; // skip version
        Some((p[0], u16::from_be_bytes([p[1], p[2]]), u16::from_be_bytes([p[3], p[4]])))
    };
    assert_eq!(density(&src), density(&enc.bytes));
    assert_eq!(density(&src).map(|d| d.1), Some(300));
}

#[test]
fn progressive_and_auto_mode_switches_are_honoured() {
    let src = fixture("baseline_plain.jpg");
    let is_prog = |d: &[u8]| d.windows(2).any(|w| w == [0xFF, 0xC2]);

    let prog = settings(&["--jpeg-mode", "progressive"]);
    let ep = jpeg::optimize(&ctx(&prog), &src).unwrap();
    assert!(is_prog(&ep.bytes));
    assert_eq!(
        jpeg::verify_optimized(&ctx(&prog), &src, &ep.bytes, ep.lossless).unwrap(),
        Verification::CoefficientsIdentical
    );

    let base = settings(&["--jpeg-mode", "baseline"]);
    let eb = jpeg::optimize(&ctx(&base), &fixture("progressive.jpg")).unwrap();
    assert!(!is_prog(&eb.bytes));

    let auto = settings(&["--jpeg-mode", "auto"]);
    let ea = jpeg::optimize(&ctx(&auto), &src).unwrap();
    let eb_plain = jpeg::optimize(&ctx(&base), &src).unwrap();
    assert_eq!(ea.bytes.len(), ep.bytes.len().min(eb_plain.bytes.len()));
    assert_eq!(
        jpeg::verify_optimized(&ctx(&auto), &src, &ea.bytes, ea.lossless).unwrap(),
        Verification::CoefficientsIdentical
    );
}

#[test]
fn data_after_eoi_is_never_silently_dropped() {
    let s = default_settings();
    let mut src = fixture("baseline_plain.jpg");
    src.extend_from_slice(b"\x01APPENDED-PAYLOAD");
    assert!(jpeg::optimize(&ctx(&s), &src).is_err());
}

#[test]
fn garbage_and_truncated_files_fail_cleanly() {
    let s = default_settings();
    assert!(jpeg::optimize(&ctx(&s), b"not a jpeg at all").is_err());
    let src = fixture("baseline_plain.jpg");
    assert!(jpeg::optimize(&ctx(&s), &src[..src.len() / 2]).is_err());
}

#[test]
fn quality_recompresses_without_requiring_lossy_and_falls_back_to_lossless_when_smaller() {
    // Low quality shrinks the image -> lossy recompression is chosen (jpegoptim -m60).
    let s_low = settings(&["--quality", "60"]);
    let src = fixture("baseline_plain.jpg");
    let enc = jpeg::optimize(&ctx(&s_low), &src).unwrap();
    assert!(!enc.lossless);
    assert!(enc.bytes.len() < src.len());
    assert_eq!(
        jpeg::verify_optimized(&ctx(&s_low), &src, &enc.bytes, enc.lossless).unwrap(),
        Verification::DecodableOnly
    );

    // High quality ceiling on an already-compressed JPEG -> lossless is smaller, so jpegoptim
    // keeps the lossless result and avoids needless generation loss.
    let s_high = settings(&["--quality", "98"]);
    let enc_high = jpeg::optimize(&ctx(&s_high), &enc.bytes).unwrap();
    assert!(enc_high.lossless, "lossless fallback must be used when lossless <= lossy");
    assert!(enc_high.notes.iter().any(|n| n.contains("jpegoptim --max")));
    assert_eq!(
        jpeg::verify_optimized(&ctx(&s_high), &enc.bytes, &enc_high.bytes, enc_high.lossless).unwrap(),
        Verification::CoefficientsIdentical
    );
}

#[test]
fn mozjpeg_conversion_works_by_default_and_flattens_alpha_on_white() {
    let s = settings(&["--format", "jpeg"]);
    let mut r = pattern(24, 24, 4, 8);
    if let image_optimizer::image::color::Samples::U8(v) = &mut r.samples {
        for px in v.chunks_mut(4) {
            px[3] = 0; // everything transparent -> white
        }
    }
    let enc = jpeg::encode(&ctx(&s), &r, &Default::default()).unwrap();
    assert!(!enc.lossless);
    assert!(enc.settings.contains("quality=75"), "MozJPEG cjpeg default quality is 75: {}", enc.settings);
    assert!(enc.notes.iter().any(|n| n.contains("transparency")));
    let d = mozjpeg::decode(&enc.bytes, 1 << 26).unwrap();
    if let image_optimizer::image::color::Samples::U8(v) = &d.raster.samples {
        assert!(v.iter().all(|&x| x >= 250), "fully transparent area must become white");
    } else {
        panic!("8-bit expected");
    }
}

#[test]
fn mozjpeg_conversion_keeps_metadata() {
    let s = settings(&["--quality", "85"]);
    let mut m = metadata(3);
    m.icc = Some(fixture_icc());
    let enc = jpeg::encode(&ctx(&s), &pattern(32, 32, 3, 8), &m).unwrap();
    let d = mozjpeg::decode(&enc.bytes, 1 << 26).unwrap();
    assert!(d.metadata.exif.is_some() && d.metadata.xmp.is_some());
    assert_eq!(d.metadata.icc, m.icc);
}

fn fixture_icc() -> Vec<u8> {
    let src = fixture("baseline_meta.jpg");
    mozjpeg::decode(&src, 1 << 26).unwrap().metadata.icc.expect("fixture has ICC")
}
