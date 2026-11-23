mod common;
use common::*;
use image_optimizer::engines::libwebp;
use image_optimizer::formats::{webp, Verification};
use image_optimizer::image::color::Samples;
use image_optimizer::image::identification::webp_is_lossless;

#[test]
fn default_mode_matches_cwebp_lossy_q75_m4() {
    let s = settings(&[]);
    let c = ctx(&s);
    let r = pattern(64, 64, 3, 8);
    let enc = webp::encode(&c, &r, &Default::default()).unwrap();
    assert!(!enc.lossless);
    assert!(enc.settings.contains("quality=75"), "cwebp default quality is 75: {}", enc.settings);
    assert!(enc.settings.contains("method=4"), "cwebp default method is 4: {}", enc.settings);
    assert_eq!(webp_is_lossless(&enc.bytes), Some(false));
    assert_eq!(webp::verify(&c, &r, &enc.bytes, enc.lossless).unwrap(), Verification::DecodableOnly);
}

#[test]
fn lossless_and_exact_and_lossless_preset_match_cwebp() {
    // --lossless with --exact preserves RGB under alpha=0 (strict pixel-identical).
    let s_exact = settings(&["--lossless", "--exact", "-q", "100", "-m", "6"]);
    let c = ctx(&s_exact);
    let mut r = pattern(24, 24, 4, 8);
    if let Samples::U8(v) = &mut r.samples {
        for px in v.chunks_mut(4).step_by(2) {
            px[3] = 0;
        }
    }
    let enc = webp::encode(&c, &r, &Default::default()).unwrap();
    assert!(enc.lossless);
    assert_eq!(webp_is_lossless(&enc.bytes), Some(true));
    let d = libwebp::decode(&enc.bytes, 1 << 26).unwrap();
    assert_eq!(rgba16(&d.raster), rgba16(&r), "--exact must keep RGB of transparent pixels");
    assert_eq!(webp::verify(&c, &r, &enc.bytes, enc.lossless).unwrap(), Verification::PixelIdentical);

    // --lossless without --exact (cwebp default) still verifies visible pixels & alpha.
    let s_def_ll = settings(&["--lossless"]);
    let enc_ll = webp::encode(&ctx(&s_def_ll), &r, &Default::default()).unwrap();
    assert_eq!(webp_is_lossless(&enc_ll.bytes), Some(true));
    let ver = webp::verify(&ctx(&s_def_ll), &r, &enc_ll.bytes, enc_ll.lossless).unwrap();
    assert!(matches!(ver, Verification::PixelIdentical | Verification::VisiblePixelsIdentical));

    // --lossless-preset 9 (cwebp -z 9) enables lossless automatically.
    let s_z9 = settings(&["--lossless-preset", "9"]);
    let enc_z9 = webp::encode(&ctx(&s_z9), &pattern(32, 32, 3, 8), &Default::default()).unwrap();
    assert!(enc_z9.lossless);
    assert!(enc_z9.settings.contains("preset=9"));
    assert_eq!(webp_is_lossless(&enc_z9.bytes), Some(true));
}

#[test]
fn gray_is_expanded_exactly() {
    let s = settings(&["--lossless"]);
    let c = ctx(&s);
    let r = pattern(17, 11, 1, 8);
    let enc = webp::encode(&c, &r, &Default::default()).unwrap();
    assert_eq!(webp::verify(&c, &r, &enc.bytes, enc.lossless).unwrap(), Verification::PixelIdentical);
}

#[test]
fn sixteen_bit_source_is_reduced_by_default_and_refused_in_lossless_mode() {
    let r = pattern(16, 16, 3, 16);
    let lossless = settings(&["--lossless"]);
    assert!(webp::encode(&ctx(&lossless), &r, &Default::default()).is_err());
    let def = settings(&[]);
    let enc = webp::encode(&ctx(&def), &r, &Default::default()).unwrap();
    assert!(enc.notes.iter().any(|n| n.contains("16-bit")));
}

#[test]
fn quality_method_and_sharp_yuv_match_cwebp_flags() {
    let s = settings(&["-q", "80", "-m", "6", "--sharp_yuv", "--alpha_q=90"]);
    let c = ctx(&s);
    let r = pattern(64, 64, 3, 8);
    let enc = webp::encode(&c, &r, &Default::default()).unwrap();
    assert!(!enc.lossless);
    assert!(enc.settings.contains("quality=80"));
    assert!(enc.settings.contains("method=6"));
    assert!(enc.settings.contains("alpha_q=90"));
    assert!(enc.settings.contains("sharp_yuv=true"));
    assert_eq!(webp_is_lossless(&enc.bytes), Some(false));
    assert_eq!(webp::verify(&c, &r, &enc.bytes, enc.lossless).unwrap(), Verification::DecodableOnly);
}

#[test]
fn metadata_roundtrips_and_strip_options_work() {
    let r = pattern(32, 32, 3, 8);
    let mut m = metadata(1);
    m.icc = image_optimizer::engines::mozjpeg::decode(&fixture("baseline_meta.jpg"), 1 << 26)
        .unwrap()
        .metadata
        .icc;
    assert!(m.icc.is_some());

    let keep = settings(&["--lossless"]);
    let enc = webp::encode(&ctx(&keep), &r, &m).unwrap();
    let d = libwebp::decode(&enc.bytes, 1 << 26).unwrap();
    assert!(d.metadata.icc.is_some());
    assert!(d.metadata.exif.is_some());
    assert!(d.metadata.xmp.as_deref().map(|x| x.windows(8).any(|w| w == b"UnitTest")).unwrap_or(false));

    let strip_meta = settings(&["--lossless", "--strip-metadata"]);
    let enc = webp::encode(&ctx(&strip_meta), &r, &m).unwrap();
    let d = libwebp::decode(&enc.bytes, 1 << 26).unwrap();
    assert!(d.metadata.exif.is_none());
    assert!(d.metadata.xmp.is_none());
    assert!(d.metadata.icc.is_some(), "ICC survives --strip-metadata");

    let strip_all = settings(&["--lossless", "--strip-all"]);
    let enc = webp::encode(&ctx(&strip_all), &r, &m).unwrap();
    let d = libwebp::decode(&enc.bytes, 1 << 26).unwrap();
    assert!(d.metadata.icc.is_none() && d.metadata.exif.is_none() && d.metadata.xmp.is_none());
}

#[test]
fn corrupt_input_is_an_error_not_a_panic() {
    assert!(libwebp::decode(b"RIFF\x10\x00\x00\x00WEBPVP8 garbage", 1 << 26).is_err());
    assert!(libwebp::decode(&[], 1 << 26).is_err());
}
