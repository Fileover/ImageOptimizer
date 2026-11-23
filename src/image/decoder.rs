//! Decoding of every supported input format into a [`Raster`] plus normalized [`Metadata`].
//!
//! Animation is never silently discarded: animated PNG/WebP/JPEG XL/GIF inputs are rejected
//! with an explanatory error when a conversion would lose frames.

use super::color::{checked_len, try_zeroed, Raster, Samples};
use super::identification::Format;
use super::metadata::{normalize_exif, Metadata};
use crate::engines::{libjxl, libwebp, mozjpeg};
use crate::errors::{Error, Result};
use std::io::Cursor;

pub struct Decoded {
    pub raster: Raster,
    pub metadata: Metadata,
}

pub struct DecodeLimits {
    pub max_pixels: u64,
    pub threads: i32,
}

pub fn decode(data: &[u8], format: Format, lim: &DecodeLimits) -> Result<Decoded> {
    match format {
        Format::Png => decode_png(data, lim),
        Format::Jpeg => mozjpeg::decode(data, lim.max_pixels),
        Format::WebP => libwebp::decode(data, lim.max_pixels),
        Format::JpegXl => libjxl::decode(data, lim.max_pixels, lim.threads, false),
        Format::Bmp | Format::Tiff | Format::Gif => decode_with_image_crate(data, format, lim),
    }
}

fn check_pixels(w: u64, h: u64, lim: &DecodeLimits) -> Result<()> {
    if w == 0 || h == 0 || w * h > lim.max_pixels {
        return Err(Error::Limit(format!(
            "image dimensions {w}x{h} exceed the pixel limit ({} pixels); raise --max-pixels to allow it",
            lim.max_pixels
        )));
    }
    Ok(())
}

fn decode_png(data: &[u8], lim: &DecodeLimits) -> Result<Decoded> {
    let mut dec = png::Decoder::new(Cursor::new(data));
    // Palette -> RGB, tRNS -> alpha, <8-bit gray -> 8-bit. 16-bit stays 16-bit.
    dec.set_transformations(png::Transformations::EXPAND);
    let mut reader = dec.read_info().map_err(|e| Error::codec("png", e.to_string()))?;
    let (w, h) = {
        let i = reader.info();
        (i.width as u64, i.height as u64)
    };
    check_pixels(w, h, lim)?;
    if reader.info().animation_control.is_some() {
        return Err(Error::Unsupported(
            "animated PNG (APNG): converting would discard the animation; use --format original/png to optimize it in place"
                .into(),
        ));
    }
    let mut metadata = Metadata::default();
    {
        let info = reader.info();
        metadata.icc = info.icc_profile.as_ref().map(|c| c.to_vec());
        metadata.exif = info.exif_metadata.as_ref().map(|c| normalize_exif(c.to_vec()));
        for t in &info.utf8_text {
            if t.keyword == "XML:com.adobe.xmp" {
                if let Ok(s) = t.get_text() {
                    metadata.xmp = Some(s.into_bytes());
                }
            }
        }
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| Error::Limit("PNG output buffer size overflows".into()))?;
    let mut buf: Vec<u8> = try_zeroed(size)?;
    let frame = reader.next_frame(&mut buf).map_err(|e| Error::codec("png", e.to_string()))?;
    let (ct, bd) = reader.output_color_type();
    let channels = match ct {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(Error::codec("png", "palette was not expanded")),
    };
    let n = checked_len(frame.width, frame.height, channels)?;
    let samples = match bd {
        png::BitDepth::Eight => {
            buf.truncate(n);
            Samples::U8(buf)
        }
        png::BitDepth::Sixteen => {
            let mut v: Vec<u16> = try_zeroed(n)?;
            for (dst, c) in v.iter_mut().zip(buf.chunks_exact(2)) {
                *dst = u16::from_be_bytes([c[0], c[1]]); // PNG stores 16-bit samples big-endian
            }
            Samples::U16(v)
        }
        other => return Err(Error::codec("png", format!("unexpected output bit depth {other:?}"))),
    };
    Ok(Decoded {
        raster: Raster { width: frame.width, height: frame.height, channels, samples, is_cmyk: false },
        metadata,
    })
}

fn decode_with_image_crate(data: &[u8], format: Format, lim: &DecodeLimits) -> Result<Decoded> {
    use image::{DynamicImage, ImageDecoder};
    let fmt = match format {
        Format::Bmp => image::ImageFormat::Bmp,
        Format::Tiff => image::ImageFormat::Tiff,
        _ => image::ImageFormat::Gif,
    };
    if format == Format::Gif {
        use image::codecs::gif::GifDecoder;
        use image::AnimationDecoder;
        let d = GifDecoder::new(Cursor::new(data)).map_err(|e| Error::codec("image", e.to_string()))?;
        if d.into_frames().take(2).count() > 1 {
            return Err(Error::Unsupported(
                "animated GIF: converting would discard the animation (only single-frame GIFs are supported)".into(),
            ));
        }
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(data), fmt);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(lim.max_pixels.saturating_mul(8));
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| Error::codec("image", e.to_string()))?;
    let (w, h) = decoder.dimensions();
    check_pixels(w as u64, h as u64, lim)?;
    let icc = decoder.icc_profile().ok().flatten();
    let exif = decoder.exif_metadata().ok().flatten().map(normalize_exif);
    let img = DynamicImage::from_decoder(decoder).map_err(|e| Error::codec("image", e.to_string()))?;
    let (width, height) = (img.width(), img.height());
    let (channels, samples) = match img {
        DynamicImage::ImageLuma8(b) => (1, Samples::U8(b.into_raw())),
        DynamicImage::ImageLumaA8(b) => (2, Samples::U8(b.into_raw())),
        DynamicImage::ImageRgb8(b) => (3, Samples::U8(b.into_raw())),
        DynamicImage::ImageRgba8(b) => (4, Samples::U8(b.into_raw())),
        DynamicImage::ImageLuma16(b) => (1, Samples::U16(b.into_raw())),
        DynamicImage::ImageLumaA16(b) => (2, Samples::U16(b.into_raw())),
        DynamicImage::ImageRgb16(b) => (3, Samples::U16(b.into_raw())),
        DynamicImage::ImageRgba16(b) => (4, Samples::U16(b.into_raw())),
        other => {
            return Err(Error::Unsupported(format!(
                "{} sample layout {:?} cannot be converted without loss",
                format.name(),
                other.color()
            )))
        }
    };
    Ok(Decoded {
        raster: Raster { width, height, channels, samples, is_cmyk: false },
        metadata: Metadata { icc, exif, xmp: None },
    })
}
