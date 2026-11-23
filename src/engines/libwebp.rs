//! libwebp engine (WebPConfig/WebPPicture encoder, libwebpmux metadata, libwebpdemux decode).

use super::ffi::*;
use super::{borrowed_image, decoded_from, map_rc};
use crate::errors::{Error, Result};
use crate::image::color::Raster;
use crate::image::decoder::Decoded;
use crate::image::metadata::Metadata;

#[derive(Clone, Debug)]
pub struct WebpOptions {
    pub lossless: bool,
    /// 0..=100. In lossless mode this trades speed for size (like `cwebp -z`/`-q`).
    pub quality: f32,
    /// 0..=6 (`cwebp -m`).
    pub method: i32,
    /// 0..=100 (`cwebp -alpha_q`).
    pub alpha_quality: i32,
    pub threads: i32,
    /// Lossy RGB->YUV sharp conversion (`cwebp -sharp_yuv`).
    pub sharp_yuv: bool,
}

pub fn version() -> String {
    // SAFETY: pure function returning a packed version number.
    let v = unsafe { shim_webp_encoder_version() };
    format!("{}.{}.{}", (v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff)
}

/// `raster` must be 8-bit RGB or RGBA.
pub fn encode(raster: &Raster, meta: &Metadata, o: &WebpOptions) -> Result<Vec<u8>> {
    let img = borrowed_image(raster, meta);
    let params = WebpParams {
        lossless: o.lossless as i32,
        quality: o.quality,
        method: o.method,
        alpha_quality: o.alpha_quality,
        threads: o.threads,
        // Lossless keeps the RGB values hidden under fully transparent pixels.
        exact: o.lossless as i32,
        sharp_yuv: (!o.lossless && o.sharp_yuv) as i32,
    };
    let mut out = OwnedBuffer::new();
    let mut err = ErrBuf::new();
    // SAFETY: `img` borrows data that outlives the call; `out`/`err` are valid for writes.
    let rc = unsafe { shim_webp_encode(&img, &params, &mut out.0, err.as_mut_ptr()) };
    if rc != IO_OK {
        return Err(map_rc("libwebp", rc, &err));
    }
    Ok(out.to_vec())
}

pub fn decode(data: &[u8], max_pixels: u64) -> Result<Decoded> {
    let mut img = OwnedImage::new();
    let mut err = ErrBuf::new();
    // SAFETY: `data` is valid for `len` bytes; `img` and `err` are valid for writes.
    let rc = unsafe { shim_webp_decode(data.as_ptr(), data.len(), max_pixels, &mut img.0, err.as_mut_ptr()) };
    if rc != IO_OK {
        return Err(map_rc("libwebp", rc, &err));
    }
    decoded_from(&img).map_err(|e| match e {
        Error::Invalid(m) => Error::codec("libwebp", m),
        e => e,
    })
}
