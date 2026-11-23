//! MozJPEG engine used for JPEG pixel decoding and for encoding converted images to JPEG.
//! (Lossless JPEG optimization is `engines::jpegoptim`.)

use super::ffi::*;
use super::{borrowed_image, decoded_from, map_rc};
use crate::errors::Result;
use crate::image::color::Raster;
use crate::image::decoder::Decoded;
use crate::image::metadata::Metadata;
use std::ffi::CStr;

pub fn version() -> String {
    // SAFETY: returns a pointer to a static NUL-terminated string.
    unsafe { CStr::from_ptr(shim_jpeg_library_version()) }.to_string_lossy().into_owned()
}

pub fn decode(data: &[u8], max_pixels: u64) -> Result<Decoded> {
    let mut img = OwnedImage::new();
    let mut err = ErrBuf::new();
    // SAFETY: `data` valid for `len`; `img`/`err` valid for writes.
    let rc = unsafe { shim_jpeg_decode(data.as_ptr(), data.len(), max_pixels, &mut img.0, err.as_mut_ptr()) };
    if rc != IO_OK {
        return Err(map_rc("mozjpeg", rc, &err));
    }
    decoded_from(&img)
}

/// `raster` must be 8-bit gray or RGB.
pub fn encode(raster: &Raster, meta: &Metadata, quality: u8, progressive: bool) -> Result<Vec<u8>> {
    let img = borrowed_image(raster, meta);
    let p = JpegEncParams { quality: quality as i32, progressive: progressive as i32 };
    let mut out = OwnedBuffer::new();
    let mut err = ErrBuf::new();
    // SAFETY: all pointers valid for the duration of the call.
    let rc = unsafe { shim_jpeg_encode(&img, &p, &mut out.0, err.as_mut_ptr()) };
    if rc != IO_OK {
        return Err(map_rc("mozjpeg", rc, &err));
    }
    Ok(out.to_vec())
}
