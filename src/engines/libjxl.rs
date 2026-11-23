//! libjxl engine (public C API: JxlEncoder / JxlDecoder / thread parallel runner).

use super::ffi::*;
use super::{borrowed_image, decoded_from, map_rc};
use crate::errors::Result;
use crate::image::color::Raster;
use crate::image::decoder::Decoded;
use crate::image::metadata::{set_exif_orientation, Metadata};

#[derive(Clone, Debug)]
pub struct JxlOptions {
    pub lossless: bool,
    /// Butteraugli distance (lossy only). 0.0 is rejected: use `lossless`.
    pub distance: f32,
    /// 1..=11 (11 automatically enables `JxlEncoderAllowExpertOptions`).
    pub effort: i32,
    /// Brotli compression effort for metadata boxes and JPEG transcoding (`-1` default, `0..=11`).
    pub brotli_effort: i32,
    pub threads: i32,
    pub store_metadata: bool,
}

pub fn encoder_version() -> String {
    // SAFETY: pure function.
    let v = unsafe { shim_jxl_encoder_version() };
    format!("{}.{}.{}", v / 1_000_000, (v / 1000) % 1000, v % 1000)
}

pub fn decoder_version() -> String {
    // SAFETY: pure function.
    let v = unsafe { shim_jxl_decoder_version() };
    format!("{}.{}.{}", v / 1_000_000, (v / 1000) % 1000, v % 1000)
}

/// libjxl's recommended quality (0..100) -> Butteraugli distance mapping.
pub fn distance_from_quality(q: f32) -> f32 {
    // SAFETY: pure function.
    unsafe { shim_jxl_distance_from_quality(q) }
}

fn params(o: &JxlOptions, orientation: u16) -> JxlParams {
    JxlParams {
        lossless: o.lossless as i32,
        distance: o.distance,
        effort: o.effort,
        brotli_effort: o.brotli_effort,
        threads: o.threads,
        orientation: orientation as i32,
        store_metadata: o.store_metadata as i32,
    }
}

pub fn encode(raster: &Raster, meta: &Metadata, o: &JxlOptions) -> Result<Vec<u8>> {
    let img = borrowed_image(raster, meta);
    // Pixels are stored un-rotated; the EXIF orientation is mirrored into the codestream
    // header because JPEG XL decoders ignore the orientation inside the Exif box.
    let orientation = if o.store_metadata { meta.orientation() } else { 1 };
    let p = params(o, orientation);
    let mut out = OwnedBuffer::new();
    let mut err = ErrBuf::new();
    // SAFETY: all pointers are valid for the duration of the call.
    let rc = unsafe { shim_jxl_encode(&img, &p, &mut out.0, err.as_mut_ptr()) };
    if rc != IO_OK {
        return Err(map_rc("libjxl", rc, &err));
    }
    Ok(out.to_vec())
}

/// Lossless JPEG transcoding. Fails (never downgrades) if libjxl cannot reconstruct the JPEG.
pub fn encode_jpeg(jpeg: &[u8], o: &JxlOptions) -> Result<Vec<u8>> {
    let p = params(o, 1);
    let mut out = OwnedBuffer::new();
    let mut err = ErrBuf::new();
    // SAFETY: `jpeg` is valid for `len` bytes; `out`/`err` valid for writes.
    let rc = unsafe { shim_jxl_encode_jpeg(jpeg.as_ptr(), jpeg.len(), &p, &mut out.0, err.as_mut_ptr()) };
    if rc != IO_OK {
        return Err(map_rc("libjxl", rc, &err));
    }
    Ok(out.to_vec())
}

/// `keep_orientation = false` returns upright pixels and resets the EXIF orientation to 1 so
/// that converted files are never rotated twice.
pub fn decode(data: &[u8], max_pixels: u64, threads: i32, keep_orientation: bool) -> Result<Decoded> {
    let mut img = OwnedImage::new();
    let mut err = ErrBuf::new();
    // SAFETY: see other FFI calls in this module.
    let rc = unsafe {
        shim_jxl_decode(
            data.as_ptr(),
            data.len(),
            max_pixels,
            threads,
            keep_orientation as i32,
            &mut img.0,
            err.as_mut_ptr(),
        )
    };
    if rc != IO_OK {
        return Err(map_rc("libjxl", rc, &err));
    }
    let mut d = decoded_from(&img)?;
    if !keep_orientation {
        if let Some(e) = d.metadata.exif.as_mut() {
            set_exif_orientation(e, 1);
        }
    }
    Ok(d)
}

/// Rebuilds the original JPEG from a transcoded `.jxl`. `Ok(None)` = no reconstruction data.
pub fn decode_jpeg(data: &[u8], threads: i32) -> Result<Option<Vec<u8>>> {
    let mut out = OwnedBuffer::new();
    let mut err = ErrBuf::new();
    // SAFETY: see other FFI calls in this module.
    let rc = unsafe { shim_jxl_decode_jpeg(data.as_ptr(), data.len(), threads, &mut out.0, err.as_mut_ptr()) };
    match rc {
        IO_OK => Ok(Some(out.to_vec())),
        IO_ERR_NO_RECON => Ok(None),
        rc => Err(map_rc("libjxl", rc, &err)),
    }
}
