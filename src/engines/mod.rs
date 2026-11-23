//! Native codec engines. Each submodule wraps exactly one upstream implementation behind a
//! small safe Rust API; no upstream type crosses these boundaries.
//!
//! * `libwebp`    - webmproject/libwebp (encode + decode + mux)
//! * `libjxl`     - libjxl/libjxl (encode, lossless JPEG transcoding, decode)
//! * `mozjpeg`    - MozJPEG (JPEG pixel decode / encode for conversions)
//! * `jpegoptim`  - jpegoptim optimization core (lossless Huffman/progressive optimization)
//! * `oxipng`     - OxiPNG library with Zopfli

pub mod ffi;
pub mod jpegoptim;
pub mod libjxl;
pub mod libwebp;
pub mod mozjpeg;
pub mod oxipng;

use crate::errors::Error;
use crate::image::color::Raster;
use crate::image::decoder::Decoded;
use crate::image::metadata::{normalize_exif, Metadata};
use ffi::{buf_slice, ErrBuf, IoBuffer, IoImage, OwnedImage, IO_ERR_TOO_LARGE, IO_ERR_UNSUPPORTED};

/// Maps a shim return code and message to the application error type.
pub(crate) fn map_rc(engine: &'static str, rc: i32, err: &ErrBuf) -> Error {
    let msg = err.message();
    match rc {
        IO_ERR_UNSUPPORTED => Error::Unsupported(format!("{engine}: {msg}")),
        IO_ERR_TOO_LARGE => Error::Limit(format!("{engine}: {msg}")),
        _ => Error::codec(engine, msg),
    }
}

/// Builds a *borrowed* `IoImage` view over a raster and its metadata. The result must not be
/// released through the shim: the shims only read from it.
pub(crate) fn borrowed_image(r: &Raster, m: &Metadata) -> IoImage {
    fn view(d: &Option<Vec<u8>>) -> IoBuffer {
        match d {
            Some(v) if !v.is_empty() => IoBuffer { data: v.as_ptr() as *mut u8, len: v.len() },
            _ => IoBuffer::empty(),
        }
    }
    let bytes = r.as_bytes();
    IoImage {
        width: r.width,
        height: r.height,
        channels: r.channels,
        bit_depth: r.depth(),
        pixels: bytes.as_ptr() as *mut u8,
        pixels_len: bytes.len(),
        icc: view(&m.icc),
        exif: view(&m.exif),
        xmp: view(&m.xmp),
        is_animated: 0,
        is_cmyk: r.is_cmyk as i32,
    }
}

/// Copies a shim-decoded image into safe Rust structures.
pub(crate) fn decoded_from(img: &OwnedImage) -> crate::errors::Result<Decoded> {
    let i = &img.0;
    let mut raster = Raster::from_native_bytes(i.width, i.height, i.channels, i.bit_depth, img.pixels())?;
    raster.is_cmyk = i.is_cmyk != 0;
    Ok(Decoded {
        raster,
        metadata: Metadata {
            icc: buf_slice(&i.icc),
            exif: buf_slice(&i.exif).map(normalize_exif),
            xmp: buf_slice(&i.xmp),
        },
    })
}
