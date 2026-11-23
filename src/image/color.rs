//! In-memory raster and lossless sample-format conversions.

use crate::errors::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Samples {
    U8(Vec<u8>),
    U16(Vec<u16>),
}

/// Interleaved raster: 1 = gray, 2 = gray+alpha, 3 = RGB, 4 = RGBA (or CMYK if `is_cmyk`).
#[derive(Clone, Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub samples: Samples,
    pub is_cmyk: bool,
}

/// Allocates a zero-filled vector, reporting failure instead of aborting the process.
pub fn try_zeroed<T: Clone + Default>(n: usize) -> Result<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n)
        .map_err(|_| Error::Limit(format!("out of memory while allocating {n} samples")))?;
    v.resize(n, T::default());
    Ok(v)
}

pub fn checked_len(width: u32, height: u32, channels: u32) -> Result<usize> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(channels as usize))
        .ok_or_else(|| Error::Limit("image dimensions overflow".into()))
}

impl Raster {
    pub fn depth(&self) -> u32 {
        match self.samples {
            Samples::U8(_) => 8,
            Samples::U16(_) => 16,
        }
    }
    pub fn has_alpha(&self) -> bool {
        !self.is_cmyk && (self.channels == 2 || self.channels == 4)
    }
    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Native-endian byte view used for FFI.
    pub fn as_bytes(&self) -> &[u8] {
        match &self.samples {
            Samples::U8(v) => v,
            // SAFETY: u16 has no padding and any byte pattern is a valid u8; the length is the
            // exact byte size of the slice and u8 alignment is 1.
            Samples::U16(v) => unsafe {
                std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v.as_slice()))
            },
        }
    }

    pub fn from_native_bytes(width: u32, height: u32, channels: u32, depth: u32, b: &[u8]) -> Result<Self> {
        let n = checked_len(width, height, channels)?;
        let samples = match depth {
            8 => {
                if b.len() != n {
                    return Err(Error::Invalid("decoder returned an unexpected buffer size".into()));
                }
                Samples::U8(b.to_vec())
            }
            16 => {
                if b.len() != n * 2 {
                    return Err(Error::Invalid("decoder returned an unexpected buffer size".into()));
                }
                let mut v: Vec<u16> = try_zeroed(n)?;
                for (dst, c) in v.iter_mut().zip(b.chunks_exact(2)) {
                    *dst = u16::from_ne_bytes([c[0], c[1]]);
                }
                Samples::U16(v)
            }
            d => return Err(Error::Invalid(format!("unsupported bit depth {d}"))),
        };
        Ok(Self { width, height, channels, samples, is_cmyk: false })
    }

    /// Canonical RGBA16 representation used to compare rasters of different layouts.
    pub fn to_rgba16(&self) -> Result<Vec<u16>> {
        let px = self.pixel_count() as usize;
        let mut out: Vec<u16> = try_zeroed(
            px.checked_mul(4).ok_or_else(|| Error::Limit("image dimensions overflow".into()))?,
        )?;
        let ch = self.channels as usize;
        let get = |i: usize| -> u16 {
            match &self.samples {
                Samples::U8(v) => v[i] as u16 * 257,
                Samples::U16(v) => v[i],
            }
        };
        for p in 0..px {
            let b = p * ch;
            let (r, g, bl, a) = match ch {
                1 => (get(b), get(b), get(b), 65535),
                2 => (get(b), get(b), get(b), get(b + 1)),
                3 => (get(b), get(b + 1), get(b + 2), 65535),
                _ => (get(b), get(b + 1), get(b + 2), get(b + 3)),
            };
            out[p * 4..p * 4 + 4].copy_from_slice(&[r, g, bl, a]);
        }
        Ok(out)
    }

    /// Gray -> RGB, gray+alpha -> RGBA (exact).
    pub fn expand_gray(&self) -> Result<Raster> {
        if self.channels > 2 || self.is_cmyk {
            return Ok(self.clone());
        }
        let px = self.pixel_count() as usize;
        let out_ch = if self.channels == 1 { 3 } else { 4 };
        macro_rules! expand {
            ($v:expr, $variant:ident) => {{
                let mut o = Vec::new();
                o.try_reserve_exact(px * out_ch)
                    .map_err(|_| Error::Limit("out of memory while expanding grayscale".into()))?;
                for p in 0..px {
                    if self.channels == 1 {
                        let g = $v[p];
                        o.extend_from_slice(&[g, g, g]);
                    } else {
                        let (g, a) = ($v[p * 2], $v[p * 2 + 1]);
                        o.extend_from_slice(&[g, g, g, a]);
                    }
                }
                Samples::$variant(o)
            }};
        }
        let samples = match &self.samples {
            Samples::U8(v) => expand!(v, U8),
            Samples::U16(v) => expand!(v, U16),
        };
        Ok(Raster { channels: out_ch as u32, samples, ..self.clone() })
    }

    /// Composites over white and drops the alpha channel (needed for JPEG output).
    pub fn flatten_on_white(&self) -> Result<Raster> {
        if !self.has_alpha() {
            return Ok(self.clone());
        }
        let px = self.pixel_count() as usize;
        let ch = self.channels as usize;
        let color = ch - 1;
        let mut out: Vec<u8> = Vec::new();
        out.try_reserve_exact(px * color)
            .map_err(|_| Error::Limit("out of memory while flattening alpha".into()))?;
        for p in 0..px {
            let (a, vals): (u32, Vec<u32>) = match &self.samples {
                Samples::U8(v) => {
                    (v[p * ch + color] as u32 * 257, (0..color).map(|c| v[p * ch + c] as u32 * 257).collect())
                }
                Samples::U16(v) => (v[p * ch + color] as u32, (0..color).map(|c| v[p * ch + c] as u32).collect()),
            };
            for c in vals {
                let blended = (c * a + 65535 * (65535 - a) + 32767) / 65535;
                out.push(((blended + 128) / 257).min(255) as u8);
            }
        }
        Ok(Raster { channels: color as u32, samples: Samples::U8(out), ..self.clone() })
    }

    pub fn to_8bit(&self) -> Raster {
        match &self.samples {
            Samples::U8(_) => self.clone(),
            Samples::U16(v) => Raster {
                samples: Samples::U8(v.iter().map(|&s| ((s as u32 + 128) / 257) as u8).collect()),
                ..self.clone()
            },
        }
    }

    pub fn has_transparency(&self) -> bool {
        if !self.has_alpha() {
            return false;
        }
        let ch = self.channels as usize;
        match &self.samples {
            Samples::U8(v) => v.chunks_exact(ch).any(|p| p[ch - 1] != u8::MAX),
            Samples::U16(v) => v.chunks_exact(ch).any(|p| p[ch - 1] != u16::MAX),
        }
    }
}
