#![allow(dead_code)]
use clap::Parser;
use image_optimizer::cli::Cli;
use image_optimizer::config::Settings;
use image_optimizer::formats::Ctx;
use image_optimizer::image::color::{Raster, Samples};
use image_optimizer::image::metadata::Metadata;
use std::path::PathBuf;

pub fn settings(extra: &[&str]) -> Settings {
    let mut a = vec!["ImageOptimizer", "-i", "in", "-o", "out"];
    a.extend_from_slice(extra);
    Settings::resolve(&Cli::parse_from(a)).expect("valid test settings")
}

pub fn ctx(s: &Settings) -> Ctx<'_> {
    Ctx { settings: s, threads: 1 }
}

/// Deterministic, non-trivial test pattern.
pub fn pattern(w: u32, h: u32, channels: u32, depth: u32) -> Raster {
    let n = (w * h * channels) as usize;
    let mut seed = 0x1234_5678u32;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        seed >> 16
    };
    let mut v16 = Vec::with_capacity(n);
    for y in 0..h {
        for x in 0..w {
            for c in 0..channels {
                let base = ((x * 7 + y * 3 + c * 40) % 256) as u32;
                let noise = next() % 8;
                v16.push(((base + noise).min(255)) as u16);
            }
        }
    }
    let samples = if depth == 16 {
        Samples::U16(v16.iter().map(|&s| s * 257).collect())
    } else {
        Samples::U8(v16.iter().map(|&s| s as u8).collect())
    };
    Raster { width: w, height: h, channels, samples, is_cmyk: false }
}

/// Minimal little-endian EXIF block with only the orientation tag.
pub fn exif_with_orientation(o: u16) -> Vec<u8> {
    let mut e = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, 0x01, 0x00];
    e.extend_from_slice(&[0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00]);
    e.extend_from_slice(&[o as u8, (o >> 8) as u8, 0x00, 0x00]);
    e.extend_from_slice(&[0, 0, 0, 0]);
    e
}

pub const XMP: &[u8] = b"<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>UnitTest</dc:title></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>";

pub fn metadata(orientation: u16) -> Metadata {
    Metadata { icc: None, exif: Some(exif_with_orientation(orientation)), xmp: Some(XMP.to_vec()) }
}

pub fn fixture(name: &str) -> Vec<u8> {
    let p: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name].iter().collect();
    std::fs::read(p).expect("fixture present")
}

pub fn rgba16(r: &Raster) -> Vec<u16> {
    r.to_rgba16().expect("convertible")
}
