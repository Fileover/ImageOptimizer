//! Initial PNG writer for conversions *to* PNG. The result is a valid but unoptimized PNG
//! (fast deflate) that `engines::oxipng` then recompresses; all metadata is attached here so
//! that OxiPNG only has to preserve it.

use super::color::{Raster, Samples};
use super::metadata::Metadata;
use crate::errors::{Error, Result};
use std::borrow::Cow;

pub fn write_png(r: &Raster, meta: &Metadata) -> Result<Vec<u8>> {
    if r.is_cmyk {
        return Err(Error::Unsupported("CMYK images cannot be written as PNG".into()));
    }
    let color = match r.channels {
        1 => png::ColorType::Grayscale,
        2 => png::ColorType::GrayscaleAlpha,
        3 => png::ColorType::Rgb,
        _ => png::ColorType::Rgba,
    };
    let depth = if r.depth() == 16 { png::BitDepth::Sixteen } else { png::BitDepth::Eight };

    let mut info = png::Info::with_size(r.width, r.height);
    info.color_type = color;
    info.bit_depth = depth;
    if let Some(icc) = &meta.icc {
        info.icc_profile = Some(Cow::Owned(icc.clone()));
    }
    if let Some(exif) = &meta.exif {
        info.exif_metadata = Some(Cow::Owned(exif.clone()));
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::with_info(&mut out, info).map_err(|e| Error::codec("png", e.to_string()))?;
        if let Some(xmp) = &meta.xmp {
            if let Ok(text) = String::from_utf8(xmp.clone()) {
                enc.add_itxt_chunk("XML:com.adobe.xmp".to_string(), text)
                    .map_err(|e| Error::codec("png", e.to_string()))?;
            }
        }
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().map_err(|e| Error::codec("png", e.to_string()))?;
        match &r.samples {
            Samples::U8(v) => w.write_image_data(v),
            Samples::U16(v) => {
                let mut be = Vec::new();
                be.try_reserve_exact(v.len() * 2)
                    .map_err(|_| Error::Limit("out of memory while building PNG data".into()))?;
                for s in v {
                    be.extend_from_slice(&s.to_be_bytes());
                }
                w.write_image_data(&be)
            }
        }
        .map_err(|e| Error::codec("png", e.to_string()))?;
        w.finish().map_err(|e| Error::codec("png", e.to_string()))?;
    }
    Ok(out)
}
