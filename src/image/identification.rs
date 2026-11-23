//! Format identification by file signature (never by extension) plus small container parsers
//! used for APNG detection and WebP lossless/lossy classification.

use serde::Serialize;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize)]
pub enum Format {
    Png,
    Jpeg,
    WebP,
    JpegXl,
    Bmp,
    Tiff,
    Gif,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::WebP => "WebP",
            Format::JpegXl => "JPEG XL",
            Format::Bmp => "BMP",
            Format::Tiff => "TIFF",
            Format::Gif => "GIF",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::WebP => "webp",
            Format::JpegXl => "jxl",
            Format::Bmp => "bmp",
            Format::Tiff => "tiff",
            Format::Gif => "gif",
        }
    }
    /// Sub-directory of the output folder for each *output* format.
    pub fn output_dir(self) -> &'static str {
        match self {
            Format::JpegXl => "JPEG-XL",
            Format::WebP => "WebP",
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Bmp => "BMP",
            Format::Tiff => "TIFF",
            Format::Gif => "GIF",
        }
    }
    pub fn is_output_format(self) -> bool {
        matches!(self, Format::Png | Format::Jpeg | Format::WebP | Format::JpegXl)
    }
}

pub fn identify(d: &[u8]) -> Option<Format> {
    if d.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(Format::Png)
    } else if d.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else if d.len() >= 12 && &d[0..4] == b"RIFF" && &d[8..12] == b"WEBP" {
        Some(Format::WebP)
    } else if d.starts_with(&[0xFF, 0x0A])
        || d.starts_with(&[0, 0, 0, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A])
    {
        Some(Format::JpegXl)
    } else if d.starts_with(b"BM")
        && d.len() >= 18
        && matches!(u32::from_le_bytes([d[14], d[15], d[16], d[17]]), 12 | 40 | 52 | 56 | 64 | 108 | 124)
    {
        Some(Format::Bmp)
    } else if d.starts_with(&[b'I', b'I', 42, 0]) || d.starts_with(&[b'M', b'M', 0, 42]) {
        Some(Format::Tiff)
    } else if d.starts_with(b"GIF87a") || d.starts_with(b"GIF89a") {
        Some(Format::Gif)
    } else {
        None
    }
}

/// Extensions that make an *unrecognized* file worth reporting as skipped.
pub fn has_image_extension(p: &Path) -> bool {
    matches!(
        p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref(),
        Some("png" | "jpg" | "jpeg" | "jpe" | "jfif" | "webp" | "jxl" | "bmp" | "tif" | "tiff" | "gif")
    )
}

/// Iterates over PNG chunks: (type, data). Stops at the first malformed chunk.
pub fn png_chunks(d: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut i = 8usize;
    while i + 12 <= d.len() {
        let len = u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]) as usize;
        let ty = [d[i + 4], d[i + 5], d[i + 6], d[i + 7]];
        let start = i + 8;
        let Some(end) = start.checked_add(len) else { break };
        if end + 4 > d.len() {
            break;
        }
        out.push((ty, &d[start..end]));
        i = end + 4;
    }
    out
}

/// Returns `(frames, plays)` if the PNG carries an `acTL` chunk (i.e. it is an APNG).
pub fn apng_info(d: &[u8]) -> Option<(u32, u32)> {
    for (ty, data) in png_chunks(d) {
        if &ty == b"acTL" && data.len() >= 8 {
            return Some((
                u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
                u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            ));
        }
        if &ty == b"IDAT" {
            break; // acTL must precede IDAT
        }
    }
    None
}

/// `Some(true)` for VP8L (lossless), `Some(false)` for VP8 (lossy), `None` if undetermined.
pub fn webp_is_lossless(d: &[u8]) -> Option<bool> {
    let mut i = 12usize;
    while i + 8 <= d.len() {
        let tag = &d[i..i + 4];
        let len = u32::from_le_bytes([d[i + 4], d[i + 5], d[i + 6], d[i + 7]]) as usize;
        match tag {
            b"VP8L" => return Some(true),
            b"VP8 " => return Some(false),
            _ => {}
        }
        i = i.checked_add(8)?.checked_add(len)?.checked_add(len & 1)?;
    }
    None
}

/// Fast header-only dimension peek for PNG and JPEG (accounting for JPEG EXIF orientation 5..=8).
/// Used to decide whether a bitstream-only fast path can still run when `--resize` is set.
pub fn peek_display_dimensions(d: &[u8], fmt: Format) -> Option<(u32, u32)> {
    match fmt {
        Format::Png => {
            if d.len() < 24 || &d[12..16] != b"IHDR" {
                return None;
            }
            let w = u32::from_be_bytes([d[16], d[17], d[18], d[19]]);
            let h = u32::from_be_bytes([d[20], d[21], d[22], d[23]]);
            let mut orientation = 1u16;
            for (ty, body) in png_chunks(d) {
                if &ty == b"eXIf" {
                    orientation = crate::image::metadata::exif_orientation(body).unwrap_or(1);
                    break;
                }
            }
            if w == 0 || h == 0 {
                None
            } else if orientation >= 5 {
                Some((h, w))
            } else {
                Some((w, h))
            }
        }
        Format::Jpeg => {
            if d.len() < 4 || d[0] != 0xFF || d[1] != 0xD8 {
                return None;
            }
            let mut i = 2usize;
            let mut orientation = 1u16;
            let mut dims = None;
            while i + 1 < d.len() {
                while i < d.len() && d[i] != 0xFF {
                    i += 1;
                }
                while i < d.len() && d[i] == 0xFF {
                    i += 1;
                }
                if i >= d.len() {
                    break;
                }
                let marker = d[i];
                i += 1;
                if marker == 0xD9 || marker == 0xDA {
                    break; // EOI or SOS
                }
                if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
                    continue; // standalone markers
                }
                if i + 2 > d.len() {
                    break;
                }
                let seg_len = u16::from_be_bytes([d[i], d[i + 1]]) as usize;
                if seg_len < 2 || i + seg_len > d.len() {
                    break;
                }
                let seg = &d[i + 2..i + seg_len];
                if marker == 0xE1 && seg.len() > 6 && &seg[..6] == b"Exif\0\0" && orientation == 1 {
                    orientation = crate::image::metadata::exif_orientation(&seg[6..]).unwrap_or(1);
                } else if matches!(
                    marker,
                    0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
                ) && seg.len() >= 5
                {
                    let h = u16::from_be_bytes([seg[1], seg[2]]) as u32;
                    let w = u16::from_be_bytes([seg[3], seg[4]]) as u32;
                    if w > 0 && h > 0 {
                        dims = Some((w, h));
                    }
                }
                i += seg_len;
            }
            let (w, h) = dims?;
            if orientation >= 5 { Some((h, w)) } else { Some((w, h)) }
        }
        _ => None,
    }
}
