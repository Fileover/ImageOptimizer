//! Metadata container shared by all formats, normalized to raw payloads:
//! ICC = raw profile, EXIF = raw TIFF block (`II*\0` / `MM\0*`), XMP = raw packet.

#[derive(Default, Clone, Debug)]
pub struct Metadata {
    pub icc: Option<Vec<u8>>,
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
}

impl Metadata {
    pub fn is_empty(&self) -> bool {
        self.icc.is_none() && self.exif.is_none() && self.xmp.is_none()
    }

    /// `strip_metadata` removes EXIF and XMP; ICC (which changes how colours are displayed) is
    /// removed only when `strip_icc` is requested explicitly.
    pub fn apply_strip(&mut self, strip_metadata: bool, strip_icc: bool) {
        if strip_metadata {
            self.exif = None;
            self.xmp = None;
        }
        if strip_icc {
            self.icc = None;
        }
    }

    pub fn orientation(&self) -> u16 {
        self.exif.as_deref().and_then(exif_orientation).unwrap_or(1)
    }
}

/// Removes a leading `Exif\0\0` identifier that some writers put in front of the TIFF header.
pub fn normalize_exif(mut e: Vec<u8>) -> Vec<u8> {
    if e.len() > 6 && &e[..6] == b"Exif\0\0" {
        e.drain(..6);
    }
    e
}

fn tiff_u16(d: &[u8], at: usize, le: bool) -> Option<u16> {
    let b = d.get(at..at + 2)?;
    Some(if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
}
fn tiff_u32(d: &[u8], at: usize, le: bool) -> Option<u32> {
    let b = d.get(at..at + 4)?;
    Some(if le {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    } else {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    })
}

/// Position of the orientation value inside the TIFF block plus the byte order.
fn orientation_slot(exif: &[u8]) -> Option<(usize, bool)> {
    let le = match exif.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if tiff_u16(exif, 2, le)? != 42 {
        return None;
    }
    let ifd = tiff_u32(exif, 4, le)? as usize;
    let count = tiff_u16(exif, ifd, le)? as usize;
    for i in 0..count.min(512) {
        let e = ifd.checked_add(2 + i * 12)?;
        if tiff_u16(exif, e, le)? == 0x0112 {
            return Some((e + 8, le));
        }
    }
    None
}

pub fn exif_orientation(exif: &[u8]) -> Option<u16> {
    let (pos, le) = orientation_slot(exif)?;
    let v = tiff_u16(exif, pos, le)?;
    (1..=8).contains(&v).then_some(v)
}

/// Rewrites the orientation tag in place. Returns `false` if there is no orientation tag.
pub fn set_exif_orientation(exif: &mut [u8], value: u16) -> bool {
    let Some((pos, le)) = orientation_slot(exif) else { return false };
    let b = if le { value.to_le_bytes() } else { value.to_be_bytes() };
    if let Some(dst) = exif.get_mut(pos..pos + 2) {
        dst.copy_from_slice(&b);
        true
    } else {
        false
    }
}
