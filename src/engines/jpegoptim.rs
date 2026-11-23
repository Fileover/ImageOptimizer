//! jpegoptim optimization core (GPL-3.0-or-later, adapted in `native/shims/shim_jpegoptim.c`).

use super::ffi::*;
use super::map_rc;
use crate::errors::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressiveMode {
    /// Keep the input's mode (jpegoptim default).
    Keep,
    Progressive,
    Baseline,
}

#[derive(Clone, Debug)]
pub struct JpegoptimOptions {
    /// `None` = lossless optimization. `Some(q)` = lossy recompression at quality `q`.
    pub quality: Option<u8>,
    pub progressive: ProgressiveMode,
    pub strip_com: bool,
    pub strip_exif: bool,
    pub strip_iptc: bool,
    pub strip_icc: bool,
    pub strip_xmp: bool,
    pub max_pixels: u64,
}

#[derive(Clone, Debug, Default)]
pub struct JpegReport {
    pub width: u32,
    pub height: u32,
    pub components: i32,
    pub progressive_in: bool,
    pub arithmetic_in: bool,
    pub markers_in: i32,
    pub markers_written: i32,
    pub trailing_bytes: usize,
    pub trailing_nonzero: bool,
}

pub fn optimize(data: &[u8], o: &JpegoptimOptions) -> Result<(Vec<u8>, JpegReport)> {
    let p = JpegoptimParams {
        quality: o.quality.map(|q| q as i32).unwrap_or(-1),
        progressive_mode: match o.progressive {
            ProgressiveMode::Keep => 0,
            ProgressiveMode::Progressive => 1,
            ProgressiveMode::Baseline => 2,
        },
        strip_com: o.strip_com as i32,
        strip_exif: o.strip_exif as i32,
        strip_iptc: o.strip_iptc as i32,
        strip_icc: o.strip_icc as i32,
        strip_xmp: o.strip_xmp as i32,
        max_pixels: o.max_pixels,
    };
    let mut out = OwnedBuffer::new();
    let mut info = JpegoptimInfo::default();
    let mut err = ErrBuf::new();
    // SAFETY: `data` valid for `len`; all out-pointers valid for writes.
    let rc = unsafe {
        shim_jpegoptim_optimize(data.as_ptr(), data.len(), &p, &mut out.0, &mut info, err.as_mut_ptr())
    };
    if rc != IO_OK {
        return Err(map_rc("jpegoptim", rc, &err));
    }
    let report = JpegReport {
        width: info.width,
        height: info.height,
        components: info.components,
        progressive_in: info.progressive_in != 0,
        arithmetic_in: info.arithmetic_in != 0,
        markers_in: info.markers_in,
        markers_written: info.markers_written,
        trailing_bytes: info.trailing_bytes,
        trailing_nonzero: info.trailing_nonzero != 0,
    };
    Ok((out.to_vec(), report))
}

/// Compares two JPEG files at the DCT-coefficient level (see `shim_jpeg_coefficients_equal`).
pub fn coefficients_equal(a: &[u8], b: &[u8], compare_markers: bool) -> Result<bool> {
    let mut equal = 0i32;
    let mut err = ErrBuf::new();
    // SAFETY: both slices valid for their lengths; `equal`/`err` valid for writes.
    let rc = unsafe {
        shim_jpeg_coefficients_equal(
            a.as_ptr(),
            a.len(),
            b.as_ptr(),
            b.len(),
            compare_markers as i32,
            &mut equal,
            err.as_mut_ptr(),
        )
    };
    if rc != IO_OK {
        return Err(map_rc("jpegoptim", rc, &err));
    }
    Ok(equal != 0)
}
