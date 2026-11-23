//! Raw FFI declarations for the C shims in `native/shims/io_shim.h`.
//!
//! # Safety contract (applies to every `unsafe` block in `engines/`)
//! * All structs below are `#[repr(C)]` mirrors of *flat* types declared in `io_shim.h`. No
//!   upstream library struct is ever declared in Rust.
//! * Input pointers are borrowed for the duration of the call only.
//! * Buffers returned through `IoBuffer` / `IoImage` are `malloc`ed inside the shims and are
//!   released exclusively through [`OwnedBuffer`] / [`OwnedImage`] (RAII), which call the shim
//!   release functions. Rust never frees them with its own allocator.
//! * Every function returns `0` on success; otherwise a NUL-terminated message is written into
//!   the `IO_ERR_LEN`-byte buffer supplied by the caller.

use std::os::raw::{c_char, c_float, c_int};

pub const IO_ERR_LEN: usize = 256;

pub const IO_OK: c_int = 0;
pub const IO_ERR_UNSUPPORTED: c_int = 2;
pub const IO_ERR_TOO_LARGE: c_int = 3;
pub const IO_ERR_NO_RECON: c_int = 5;

#[repr(C)]
pub struct IoBuffer {
    pub data: *mut u8,
    pub len: usize,
}

impl IoBuffer {
    pub const fn empty() -> Self {
        Self { data: std::ptr::null_mut(), len: 0 }
    }
}

#[repr(C)]
pub struct IoImage {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub bit_depth: u32,
    pub pixels: *mut u8,
    pub pixels_len: usize,
    pub icc: IoBuffer,
    pub exif: IoBuffer,
    pub xmp: IoBuffer,
    pub is_animated: c_int,
    pub is_cmyk: c_int,
}

impl IoImage {
    pub const fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            channels: 0,
            bit_depth: 0,
            pixels: std::ptr::null_mut(),
            pixels_len: 0,
            icc: IoBuffer::empty(),
            exif: IoBuffer::empty(),
            xmp: IoBuffer::empty(),
            is_animated: 0,
            is_cmyk: 0,
        }
    }
}

#[repr(C)]
pub struct WebpParams {
    pub lossless: c_int,
    pub lossless_preset: c_int,
    pub quality: c_float,
    pub method: c_int,
    pub alpha_quality: c_int,
    pub threads: c_int,
    pub exact: c_int,
    pub sharp_yuv: c_int,
}

#[repr(C)]
pub struct JxlParams {
    pub lossless: c_int,
    pub distance: c_float,
    pub effort: c_int,
    pub brotli_effort: c_int,
    pub threads: c_int,
    pub orientation: c_int,
    pub store_metadata: c_int,
}

#[repr(C)]
pub struct JpegEncParams {
    pub quality: c_int,
    pub progressive: c_int,
}

#[repr(C)]
pub struct JpegoptimParams {
    pub quality: c_int,
    pub progressive_mode: c_int,
    pub strip_com: c_int,
    pub strip_exif: c_int,
    pub strip_iptc: c_int,
    pub strip_icc: c_int,
    pub strip_xmp: c_int,
    pub max_pixels: u64,
}

#[repr(C)]
#[derive(Default)]
pub struct JpegoptimInfo {
    pub width: u32,
    pub height: u32,
    pub components: c_int,
    pub progressive_in: c_int,
    pub arithmetic_in: c_int,
    pub saw_jfif: c_int,
    pub saw_adobe: c_int,
    pub markers_in: c_int,
    pub markers_written: c_int,
    pub trailing_bytes: usize,
    pub trailing_nonzero: c_int,
}

extern "C" {
    pub fn io_free(p: *mut std::ffi::c_void);
    pub fn io_buffer_release(b: *mut IoBuffer);
    pub fn io_image_release(img: *mut IoImage);
    pub fn io_buffer_set(b: *mut IoBuffer, data: *const u8, len: usize) -> c_int;

    pub fn shim_webp_encoder_version() -> u32;
    pub fn shim_webp_decode(
        data: *const u8,
        len: usize,
        max_pixels: u64,
        out: *mut IoImage,
        err: *mut c_char,
    ) -> c_int;
    pub fn shim_webp_encode(
        input: *const IoImage,
        p: *const WebpParams,
        out: *mut IoBuffer,
        err: *mut c_char,
    ) -> c_int;

    pub fn shim_jxl_encoder_version() -> u32;
    pub fn shim_jxl_decoder_version() -> u32;
    pub fn shim_jxl_distance_from_quality(quality: c_float) -> c_float;
    pub fn shim_jxl_encode(
        input: *const IoImage,
        p: *const JxlParams,
        out: *mut IoBuffer,
        err: *mut c_char,
    ) -> c_int;
    pub fn shim_jxl_encode_jpeg(
        jpeg: *const u8,
        len: usize,
        p: *const JxlParams,
        out: *mut IoBuffer,
        err: *mut c_char,
    ) -> c_int;
    pub fn shim_jxl_decode(
        data: *const u8,
        len: usize,
        max_pixels: u64,
        threads: c_int,
        keep_orientation: c_int,
        out: *mut IoImage,
        err: *mut c_char,
    ) -> c_int;
    pub fn shim_jxl_decode_jpeg(
        data: *const u8,
        len: usize,
        threads: c_int,
        jpeg_out: *mut IoBuffer,
        err: *mut c_char,
    ) -> c_int;

    pub fn shim_jpeg_library_version() -> *const c_char;
    pub fn shim_jpeg_decode(
        data: *const u8,
        len: usize,
        max_pixels: u64,
        out: *mut IoImage,
        err: *mut c_char,
    ) -> c_int;
    pub fn shim_jpeg_encode(
        input: *const IoImage,
        p: *const JpegEncParams,
        out: *mut IoBuffer,
        err: *mut c_char,
    ) -> c_int;

    pub fn shim_jpegoptim_optimize(
        data: *const u8,
        len: usize,
        p: *const JpegoptimParams,
        out: *mut IoBuffer,
        info: *mut JpegoptimInfo,
        err: *mut c_char,
    ) -> c_int;
    pub fn shim_jpeg_coefficients_equal(
        a: *const u8,
        alen: usize,
        b: *const u8,
        blen: usize,
        compare_markers: c_int,
        equal: *mut c_int,
        err: *mut c_char,
    ) -> c_int;
}

/// RAII owner of a shim-allocated byte buffer.
pub struct OwnedBuffer(pub IoBuffer);

impl OwnedBuffer {
    pub fn new() -> Self {
        Self(IoBuffer::empty())
    }
    pub fn as_slice(&self) -> &[u8] {
        if self.0.data.is_null() || self.0.len == 0 {
            &[]
        } else {
            // SAFETY: the shim guarantees `data` points to `len` initialized bytes that stay
            // valid until `io_buffer_release` runs in `Drop`.
            unsafe { std::slice::from_raw_parts(self.0.data, self.0.len) }
        }
    }
    pub fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }
}

impl Default for OwnedBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OwnedBuffer {
    fn drop(&mut self) {
        // SAFETY: the buffer was produced by a shim (or is empty); release is idempotent.
        unsafe { io_buffer_release(&mut self.0) }
    }
}

/// RAII owner of a shim-allocated decoded image.
pub struct OwnedImage(pub IoImage);

impl OwnedImage {
    pub fn new() -> Self {
        Self(IoImage::empty())
    }
    pub fn pixels(&self) -> &[u8] {
        if self.0.pixels.is_null() || self.0.pixels_len == 0 {
            &[]
        } else {
            // SAFETY: `pixels` holds `pixels_len` bytes allocated by the shim.
            unsafe { std::slice::from_raw_parts(self.0.pixels, self.0.pixels_len) }
        }
    }
}

impl Default for OwnedImage {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OwnedImage {
    fn drop(&mut self) {
        // SAFETY: release frees pixels and metadata buffers owned by the shim.
        unsafe { io_image_release(&mut self.0) }
    }
}

pub fn buf_slice(b: &IoBuffer) -> Option<Vec<u8>> {
    if b.data.is_null() || b.len == 0 {
        None
    } else {
        // SAFETY: see OwnedBuffer::as_slice.
        Some(unsafe { std::slice::from_raw_parts(b.data, b.len) }.to_vec())
    }
}

/// Error-message buffer helper.
pub struct ErrBuf(pub [u8; IO_ERR_LEN]);

impl ErrBuf {
    pub fn new() -> Self {
        Self([0u8; IO_ERR_LEN])
    }
    pub fn as_mut_ptr(&mut self) -> *mut c_char {
        self.0.as_mut_ptr() as *mut c_char
    }
    pub fn message(&self) -> String {
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(IO_ERR_LEN);
        String::from_utf8_lossy(&self.0[..end]).into_owned()
    }
}

impl Default for ErrBuf {
    fn default() -> Self {
        Self::new()
    }
}
