//! Downscale-only image resizing and EXIF orientation normalization.
//!
//! Never upscales: if an image already fits within the requested bounds, its dimensions are
//! preserved unchanged. Transparent images are resized in premultiplied alpha space to avoid
//! dark fringing along transparent edges.

use crate::errors::{Error, Result};
use crate::image::color::{try_zeroed, Raster, Samples};
use clap::ValueEnum;
use image::{imageops, ImageBuffer, Luma, LumaA, Rgb, Rgba};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResizeFilter {
    /// Lanczos with window 3 (highest quality downscaling, default)
    #[default]
    Lanczos3,
    /// Catmull-Rom cubic filter (sharp, fast)
    CatmullRom,
    /// Bilinear / triangle filter
    Triangle,
    /// Gaussian filter
    Gaussian,
    /// Nearest-neighbor (pixel art)
    Nearest,
}

impl ResizeFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            ResizeFilter::Lanczos3 => "lanczos3",
            ResizeFilter::CatmullRom => "catmull-rom",
            ResizeFilter::Triangle => "triangle",
            ResizeFilter::Gaussian => "gaussian",
            ResizeFilter::Nearest => "nearest",
        }
    }

    fn to_image_filter(self) -> imageops::FilterType {
        match self {
            ResizeFilter::Lanczos3 => imageops::FilterType::Lanczos3,
            ResizeFilter::CatmullRom => imageops::FilterType::CatmullRom,
            ResizeFilter::Triangle => imageops::FilterType::Triangle,
            ResizeFilter::Gaussian => imageops::FilterType::Gaussian,
            ResizeFilter::Nearest => imageops::FilterType::Nearest,
        }
    }
}

/// Downscale-only resize specification.
///
/// Supported syntax:
/// * `WxH`  (e.g. `256x256`)  - fit inside `W x H` preserving aspect ratio, downscale only
/// * `N`    (e.g. `256`)      - shorthand for `NxN`
/// * `Wx`   (e.g. `256x`)     - downscale width to at most `W`, preserving aspect ratio
/// * `xH`   (e.g. `x256`)     - downscale height to at most `H`, preserving aspect ratio
/// * `WxH!` (e.g. `256x256!`) - clamp each axis independently (`min(src_w, W) x min(src_h, H)`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResizeSpec {
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub exact: bool,
}

impl fmt::Display for ResizeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.max_width, self.max_height, self.exact) {
            (Some(w), Some(h), true) => write!(f, "{w}x{h}!"),
            (Some(w), Some(h), false) => write!(f, "{w}x{h}"),
            (Some(w), None, _) => write!(f, "{w}x"),
            (None, Some(h), _) => write!(f, "x{h}"),
            (None, None, _) => write!(f, "none"),
        }
    }
}

impl Serialize for ResizeSpec {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

fn parse_dim(s: &str, raw: &str) -> Result<Option<u32>> {
    if s.is_empty() {
        return Ok(None);
    }
    let v: u32 = s
        .parse()
        .map_err(|_| Error::Invalid(format!("invalid --resize value '{raw}' (expected e.g. 256x256, 256x, x256, 256x256!)")))?;
    if v == 0 {
        return Ok(None);
    }
    Ok(Some(v))
}

impl FromStr for ResizeSpec {
    type Err = Error;

    fn from_str(raw: &str) -> Result<Self> {
        let s = raw.trim();
        if s.is_empty() {
            return Err(Error::Invalid("--resize value cannot be empty".into()));
        }
        let (body, exact) = match s.strip_suffix('!') {
            Some(b) => (b, true),
            None => (s, false),
        };
        let lower = body.to_ascii_lowercase();
        let (max_width, max_height) = if let Some((w_str, h_str)) = lower.split_once('x') {
            if h_str.contains('x') {
                return Err(Error::Invalid(format!("invalid --resize value '{raw}'")));
            }
            (parse_dim(w_str, raw)?, parse_dim(h_str, raw)?)
        } else {
            let n = parse_dim(&lower, raw)?.ok_or_else(|| {
                Error::Invalid(format!("--resize dimensions must be greater than 0 (got '{raw}')"))
            })?;
            (Some(n), Some(n))
        };

        if max_width.is_none() && max_height.is_none() {
            return Err(Error::Invalid(format!("--resize dimensions must be greater than 0 (got '{raw}')")));
        }
        if exact && (max_width.is_none() || max_height.is_none()) {
            return Err(Error::Invalid(format!(
                "--resize '{raw}': '!' exact mode requires both width and height (e.g. 256x256!)"
            )));
        }
        Ok(ResizeSpec { max_width, max_height, exact })
    }
}

impl ResizeSpec {
    /// Computes the downscaled target dimensions, or `None` if the image already fits and
    /// should not be resized (never upscales).
    pub fn target_dimensions(&self, src_w: u32, src_h: u32) -> Option<(u32, u32)> {
        if src_w == 0 || src_h == 0 {
            return None;
        }
        if self.exact {
            let mw = self.max_width.unwrap_or(src_w);
            let mh = self.max_height.unwrap_or(src_h);
            let nw = src_w.min(mw).max(1);
            let nh = src_h.min(mh).max(1);
            return if nw == src_w && nh == src_h { None } else { Some((nw, nh)) };
        }
        match (self.max_width, self.max_height) {
            (Some(mw), Some(mh)) => {
                if src_w <= mw && src_h <= mh {
                    return None;
                }
                let scale = (mw as f64 / src_w as f64).min(mh as f64 / src_h as f64);
                let nw = ((src_w as f64 * scale).round() as u32).clamp(1, mw);
                let nh = ((src_h as f64 * scale).round() as u32).clamp(1, mh);
                if nw == src_w && nh == src_h { None } else { Some((nw, nh)) }
            }
            (Some(mw), None) => {
                if src_w <= mw {
                    return None;
                }
                let scale = mw as f64 / src_w as f64;
                let nw = mw.max(1);
                let nh = ((src_h as f64 * scale).round() as u32).max(1);
                Some((nw, nh))
            }
            (None, Some(mh)) => {
                if src_h <= mh {
                    return None;
                }
                let scale = mh as f64 / src_h as f64;
                let nh = mh.max(1);
                let nw = ((src_w as f64 * scale).round() as u32).max(1);
                Some((nw, nh))
            }
            (None, None) => None,
        }
    }
}

/// Applies EXIF orientation (1..=8) to a raster so that its stored pixels are upright.
pub fn apply_orientation(r: &Raster, orientation: u16) -> Result<Raster> {
    if !(2..=8).contains(&orientation) {
        return Ok(r.clone());
    }
    let w = r.width as usize;
    let h = r.height as usize;
    let ch = r.channels as usize;
    let (nw, nh) = if orientation >= 5 { (h, w) } else { (w, h) };
    let total = nw
        .checked_mul(nh)
        .and_then(|v| v.checked_mul(ch))
        .ok_or_else(|| Error::Limit("image dimensions overflow while rotating".into()))?;

    macro_rules! transform {
        ($src:expr, $variant:ident) => {{
            let mut dst = try_zeroed(total)?;
            for sy in 0..h {
                for sx in 0..w {
                    let (dx, dy) = match orientation {
                        2 => (w - 1 - sx, sy),
                        3 => (w - 1 - sx, h - 1 - sy),
                        4 => (sx, h - 1 - sy),
                        5 => (sy, sx),
                        6 => (h - 1 - sy, sx),
                        7 => (h - 1 - sy, w - 1 - sx),
                        8 => (sy, w - 1 - sx),
                        _ => (sx, sy),
                    };
                    let s_off = (sy * w + sx) * ch;
                    let d_off = (dy * nw + dx) * ch;
                    dst[d_off..d_off + ch].copy_from_slice(&$src[s_off..s_off + ch]);
                }
            }
            Samples::$variant(dst)
        }};
    }

    let samples = match &r.samples {
        Samples::U8(v) => transform!(v, U8),
        Samples::U16(v) => transform!(v, U16),
    };
    Ok(Raster { width: nw as u32, height: nh as u32, channels: r.channels, samples, is_cmyk: r.is_cmyk })
}

fn premultiply_u8_to_u16(v: &[u8], ch: usize) -> Result<Vec<u16>> {
    let mut out = try_zeroed(v.len())?;
    let color = ch - 1;
    for (src, dst) in v.chunks_exact(ch).zip(out.chunks_exact_mut(ch)) {
        let a = src[color] as u32 * 257;
        for c in 0..color {
            let s = src[c] as u32 * 257;
            dst[c] = ((s * a + 32767) / 65535) as u16;
        }
        dst[color] = a as u16;
    }
    Ok(out)
}

fn unpremultiply_u16_to_u8(v: &[u16], ch: usize) -> Result<Vec<u8>> {
    let mut out = try_zeroed(v.len())?;
    let color = ch - 1;
    for (src, dst) in v.chunks_exact(ch).zip(out.chunks_exact_mut(ch)) {
        let a = src[color] as u32;
        if a == 0 {
            for d in dst.iter_mut() {
                *d = 0;
            }
        } else {
            for c in 0..color {
                let un = ((src[c] as u32 * 65535 + a / 2) / a).min(65535);
                dst[c] = ((un + 128) / 257).min(255) as u8;
            }
            dst[color] = ((a + 128) / 257).min(255) as u8;
        }
    }
    Ok(out)
}

fn premultiply_u16(v: &[u16], ch: usize) -> Result<Vec<u16>> {
    let mut out = try_zeroed(v.len())?;
    let color = ch - 1;
    for (src, dst) in v.chunks_exact(ch).zip(out.chunks_exact_mut(ch)) {
        let a = src[color] as u32;
        for c in 0..color {
            dst[c] = ((src[c] as u32 * a + 32767) / 65535) as u16;
        }
        dst[color] = src[color];
    }
    Ok(out)
}

fn unpremultiply_u16(v: &[u16], ch: usize) -> Result<Vec<u16>> {
    let mut out = try_zeroed(v.len())?;
    let color = ch - 1;
    for (src, dst) in v.chunks_exact(ch).zip(out.chunks_exact_mut(ch)) {
        let a = src[color] as u32;
        if a == 0 {
            for d in dst.iter_mut() {
                *d = 0;
            }
        } else {
            for c in 0..color {
                dst[c] = ((src[c] as u32 * 65535 + a / 2) / a).min(65535) as u16;
            }
            dst[color] = src[color];
        }
    }
    Ok(out)
}

fn resize_u16_buf(w: u32, h: u32, ch: u32, data: Vec<u16>, nw: u32, nh: u32, filter: imageops::FilterType) -> Result<Vec<u16>> {
    let err = || Error::Invalid("invalid raster buffer dimensions for resize".into());
    match ch {
        1 => Ok(imageops::resize(&ImageBuffer::<Luma<u16>, Vec<u16>>::from_raw(w, h, data).ok_or_else(err)?, nw, nh, filter).into_raw()),
        2 => Ok(imageops::resize(&ImageBuffer::<LumaA<u16>, Vec<u16>>::from_raw(w, h, data).ok_or_else(err)?, nw, nh, filter).into_raw()),
        3 => Ok(imageops::resize(&ImageBuffer::<Rgb<u16>, Vec<u16>>::from_raw(w, h, data).ok_or_else(err)?, nw, nh, filter).into_raw()),
        4 => Ok(imageops::resize(&ImageBuffer::<Rgba<u16>, Vec<u16>>::from_raw(w, h, data).ok_or_else(err)?, nw, nh, filter).into_raw()),
        c => Err(Error::Unsupported(format!("unsupported channel count {c} for resize"))),
    }
}

/// Resizes `r` to `new_w x new_h` using `filter`. Preserves bit depth and channel layout.
pub fn resize_raster(r: &Raster, new_w: u32, new_h: u32, filter: ResizeFilter) -> Result<Raster> {
    if r.is_cmyk {
        return Err(Error::Unsupported("CMYK images cannot be resized".into()));
    }
    if r.width == new_w && r.height == new_h {
        return Ok(r.clone());
    }
    let f = filter.to_image_filter();
    let err = || Error::Invalid("invalid raster buffer dimensions for resize".into());
    let has_trans = r.has_alpha() && r.has_transparency();

    let samples = match &r.samples {
        Samples::U8(v) => {
            if has_trans {
                let pre = premultiply_u8_to_u16(v, r.channels as usize)?;
                let resized = resize_u16_buf(r.width, r.height, r.channels, pre, new_w, new_h, f)?;
                Samples::U8(unpremultiply_u16_to_u8(&resized, r.channels as usize)?)
            } else {
                let out = match r.channels {
                    1 => imageops::resize(&ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(r.width, r.height, v.clone()).ok_or_else(err)?, new_w, new_h, f).into_raw(),
                    2 => imageops::resize(&ImageBuffer::<LumaA<u8>, Vec<u8>>::from_raw(r.width, r.height, v.clone()).ok_or_else(err)?, new_w, new_h, f).into_raw(),
                    3 => imageops::resize(&ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(r.width, r.height, v.clone()).ok_or_else(err)?, new_w, new_h, f).into_raw(),
                    4 => imageops::resize(&ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(r.width, r.height, v.clone()).ok_or_else(err)?, new_w, new_h, f).into_raw(),
                    c => return Err(Error::Unsupported(format!("unsupported channel count {c} for resize"))),
                };
                Samples::U8(out)
            }
        }
        Samples::U16(v) => {
            if has_trans {
                let pre = premultiply_u16(v, r.channels as usize)?;
                let resized = resize_u16_buf(r.width, r.height, r.channels, pre, new_w, new_h, f)?;
                Samples::U16(unpremultiply_u16(&resized, r.channels as usize)?)
            } else {
                Samples::U16(resize_u16_buf(r.width, r.height, r.channels, v.clone(), new_w, new_h, f)?)
            }
        }
    };

    Ok(Raster { width: new_w, height: new_h, channels: r.channels, samples, is_cmyk: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_valid_specs_and_rejects_invalid_ones() {
        assert_eq!("256x256".parse::<ResizeSpec>().unwrap(), ResizeSpec { max_width: Some(256), max_height: Some(256), exact: false });
        assert_eq!("256X128".parse::<ResizeSpec>().unwrap(), ResizeSpec { max_width: Some(256), max_height: Some(128), exact: false });
        assert_eq!("256".parse::<ResizeSpec>().unwrap(), ResizeSpec { max_width: Some(256), max_height: Some(256), exact: false });
        assert_eq!("256x".parse::<ResizeSpec>().unwrap(), ResizeSpec { max_width: Some(256), max_height: None, exact: false });
        assert_eq!("x256".parse::<ResizeSpec>().unwrap(), ResizeSpec { max_width: None, max_height: Some(256), exact: false });
        assert_eq!("256x256!".parse::<ResizeSpec>().unwrap(), ResizeSpec { max_width: Some(256), max_height: Some(256), exact: true });

        for bad in ["", "0", "0x0", "x", "abc", "-10x10", "256x!", "x256!", "10x20x30"] {
            assert!(bad.parse::<ResizeSpec>().is_err(), "expected error for '{bad}'");
        }
    }

    #[test]
    fn downscale_only_never_upscales() {
        let s: ResizeSpec = "256x256".parse().unwrap();
        assert_eq!(s.target_dimensions(512, 512), Some((256, 256)));
        assert_eq!(s.target_dimensions(1000, 500), Some((256, 128)));
        assert_eq!(s.target_dimensions(500, 1000), Some((128, 256)));
        assert_eq!(s.target_dimensions(256, 256), None);
        assert_eq!(s.target_dimensions(128, 128), None);
        assert_eq!(s.target_dimensions(200, 100), None);

        let w_only: ResizeSpec = "256x".parse().unwrap();
        assert_eq!(w_only.target_dimensions(512, 256), Some((256, 128)));
        assert_eq!(w_only.target_dimensions(200, 400), None);

        let h_only: ResizeSpec = "x256".parse().unwrap();
        assert_eq!(h_only.target_dimensions(512, 512), Some((256, 256)));
        assert_eq!(h_only.target_dimensions(512, 200), None);

        let exact: ResizeSpec = "256x128!".parse().unwrap();
        assert_eq!(exact.target_dimensions(512, 512), Some((256, 128)));
        assert_eq!(exact.target_dimensions(100, 512), Some((100, 128))); // width not upscaled
        assert_eq!(exact.target_dimensions(100, 100), None);
    }

    #[test]
    fn orientation_90_cw_swaps_dimensions_correctly() {
        // 2x1 RGB: left pixel red, right pixel blue.
        let r = Raster {
            width: 2,
            height: 1,
            channels: 3,
            samples: Samples::U8(vec![255, 0, 0, 0, 0, 255]),
            is_cmyk: false,
        };
        // Rotating 90 deg CW -> 1x2: top pixel red, bottom pixel blue.
        let rot = apply_orientation(&r, 6).unwrap();
        assert_eq!((rot.width, rot.height), (1, 2));
        assert_eq!(rot.samples, Samples::U8(vec![255, 0, 0, 0, 0, 255]));
    }
}
