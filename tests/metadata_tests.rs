mod common;
use common::*;
use image_optimizer::image::metadata::{exif_orientation, normalize_exif, set_exif_orientation, Metadata};

#[test]
fn exif_orientation_is_read_and_rewritten() {
    let mut e = exif_with_orientation(6);
    assert_eq!(exif_orientation(&e), Some(6));
    assert!(set_exif_orientation(&mut e, 1));
    assert_eq!(exif_orientation(&e), Some(1));
}

#[test]
fn exif_with_jpeg_prefix_is_normalized_to_a_tiff_block() {
    let mut prefixed = b"Exif\0\0".to_vec();
    prefixed.extend_from_slice(&exif_with_orientation(3));
    assert_eq!(normalize_exif(prefixed), exif_with_orientation(3));
}

#[test]
fn strip_policy_keeps_icc_unless_asked() {
    let base = Metadata { icc: Some(vec![1, 2, 3]), exif: Some(vec![4]), xmp: Some(vec![5]) };
    let mut m = base.clone();
    m.apply_strip(true, false);
    assert!(m.exif.is_none() && m.xmp.is_none() && m.icc.is_some());
    let mut m = base.clone();
    m.apply_strip(false, true);
    assert!(m.icc.is_none() && m.exif.is_some() && m.xmp.is_some());
    let mut m = base;
    m.apply_strip(false, false);
    assert!(!m.is_empty() && m.icc.is_some() && m.exif.is_some() && m.xmp.is_some());
}

#[test]
fn format_detection_uses_signatures_not_extensions() {
    use image_optimizer::image::identification::{identify, Format};
    assert_eq!(identify(&fixture("baseline_plain.jpg")), Some(Format::Jpeg));
    assert_eq!(identify(b"\x89PNG\r\n\x1a\nxxxx"), Some(Format::Png));
    assert_eq!(identify(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some(Format::WebP));
    assert_eq!(identify(b"\xFF\x0A\x00\x00"), Some(Format::JpegXl));
    assert_eq!(identify(b"\x00\x00\x00\x0CJXL \x0D\x0A\x87\x0A"), Some(Format::JpegXl));
    assert_eq!(identify(b"BM\x00\x00\x00\x00\x00\x00\x00\x00\x36\x00\x00\x00\x28\x00\x00\x00"), Some(Format::Bmp));
    assert_eq!(identify(b"II*\0\x08\0\0\0"), Some(Format::Tiff));
    assert_eq!(identify(b"GIF89a\x01\x00\x01\x00"), Some(Format::Gif));
    assert_eq!(identify(b"BM is a text file that merely starts with BM"), None);
    assert_eq!(identify(b"hello world, definitely text"), None);
}
