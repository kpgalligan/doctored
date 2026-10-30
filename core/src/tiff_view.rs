//! The ONE bounds-checked TIFF stream reader, moved out of `metadata`.
//!
//! Two entry points, two scopes:
//!
//! * [`Tiff::new`] reads a **classic** TIFF stream (version 42, 4-byte
//!   offsets) — a classic file or an EXIF blob. EXIF blobs are always
//!   classic, so the resolution probe, the export-time normalizer
//!   (`metadata_write::exif_normalized`) and the export preflight
//!   (`metadata_tiff::export_exif`) only ever see this shape.
//! * [`Tiff::file`] reads a **whole file**, classic or BigTIFF (version 43,
//!   8-byte offsets, 20-byte IFD entries with an 8-byte value field). Only
//!   the file scan (`metadata_tiff::scan`), the file orientation, the
//!   carrier header check and the float seam's planar probe use it.
//!
//! Every BigTIFF behaviour is gated on [`Tiff::big`], so a classic stream
//! parses exactly as it did before BigTIFF was read. Every 64-bit count or
//! offset converts to `usize` through `try_from` and every sum is checked:
//! a crafted value refuses the entry or the directory, never panics.
//!
//! the shared TIFF-stream reader, which lives in `tiff_view.rs` (`Tiff::new` classic files and EXIF
//! blobs only, `Tiff::file` whole files classic or BigTIFF; every BigTIFF branch gated on
//! `Tiff::big`).

use crate::metadata::Resolution;

/// Tag numbers this crate looks at inside an EXIF block. Everything else is
/// opaque.
pub(crate) const TAG_IMAGE_WIDTH: u16 = 0x0100;
pub(crate) const TAG_IMAGE_LENGTH: u16 = 0x0101;
pub(crate) const TAG_ORIENTATION: u16 = 0x0112;
pub(crate) const TAG_X_RESOLUTION: u16 = 0x011A;
pub(crate) const TAG_Y_RESOLUTION: u16 = 0x011B;
pub(crate) const TAG_RESOLUTION_UNIT: u16 = 0x0128;
pub(crate) const TAG_EXIF_IFD: u16 = 0x8769;
pub(crate) const TAG_COLOR_SPACE: u16 = 0xA001;
pub(crate) const TAG_PIXEL_X_DIMENSION: u16 = 0xA002;
pub(crate) const TAG_PIXEL_Y_DIMENSION: u16 = 0xA003;

/// TIFF field types this crate can size. An unknown code makes an ENTRY
/// unreadable, never the block: the walk skips it rather than guessing a
/// width.
pub(crate) const TYPE_SHORT: u16 = 3;
pub(crate) const TYPE_LONG: u16 = 4;
pub(crate) const TYPE_RATIONAL: u16 = 5;

/// At most this many entries per IFD. A real IFD has tens; a thousand is
/// generous and bounds the work a crafted block can ask for.
pub(crate) const MAX_IFD_ENTRIES: usize = 1000;

/// One IFD entry, located rather than decoded: 2-byte tag, 2-byte type, a
/// count (4 bytes classic, 8 BigTIFF), then a value field (4 bytes classic,
/// 8 BigTIFF) that holds the value inline when `count * type size` fits and
/// an offset otherwise.
#[derive(Clone, Copy)]
pub(crate) struct Entry {
    pub tag: u16,
    pub kind: u16,
    pub count: u64,
    /// Offset OF the 4- or 8-byte value field, from the start of the TIFF
    /// stream.
    pub field: usize,
}

/// A bounds-checked view over a TIFF stream — the ONE reader, shared by the
/// resolution probe, the TIFF file scan and the export-time normalizer in
/// `metadata_write`. Every offset in the structure is measured from byte 0
/// of the header, which is exactly why an EXIF block can be copied between
/// files intact.
pub(crate) struct Tiff<'a> {
    pub bytes: &'a [u8],
    pub big_endian: bool,
    /// BigTIFF (version 43): 8-byte offsets, counts and value fields. Only
    /// [`Tiff::file`] ever sets it.
    pub big: bool,
    /// Offset of IFD0.
    pub ifd0: usize,
}

impl<'a> Tiff<'a> {
    /// `None` unless the 8-byte header is well formed: `II` or `MM`, magic
    /// 42 in that order, and an IFD0 offset that is at least 8 and inside
    /// the block. Classic only: an EXIF blob is never BigTIFF.
    pub(crate) fn new(bytes: &'a [u8]) -> Option<Tiff<'a>> {
        let t = Tiff::ordered(bytes)?;
        if t.u16_at(2)? != 42 {
            return None;
        }
        let ifd0 = t.u32_at(4)? as usize;
        if ifd0 < 8 || ifd0 >= bytes.len() {
            return None;
        }
        Some(Tiff { ifd0, ..t })
    }

    /// A whole TIFF file: classic exactly as [`Tiff::new`], or BigTIFF —
    /// version 43, an offset bytesize of 8, a zero reserved word, and a u64
    /// IFD0 offset at byte 8 that is at least 16 and inside the file.
    pub(crate) fn file(bytes: &'a [u8]) -> Option<Tiff<'a>> {
        let t = Tiff::ordered(bytes)?;
        if t.u16_at(2)? != 43 {
            return Tiff::new(bytes);
        }
        if t.u16_at(4)? != 8 || t.u16_at(6)? != 0 {
            return None;
        }
        let ifd0 = usize::try_from(t.u64_at(8)?).ok()?;
        if ifd0 < 16 || ifd0 >= bytes.len() {
            return None;
        }
        Some(Tiff {
            ifd0,
            big: true,
            ..t
        })
    }

    /// The byte order mark alone; the caller checks the version.
    fn ordered(bytes: &'a [u8]) -> Option<Tiff<'a>> {
        let big_endian = match bytes.get(0..2)? {
            b"II" => false,
            b"MM" => true,
            _ => return None,
        };
        Some(Tiff {
            bytes,
            big_endian,
            big: false,
            ifd0: 0,
        })
    }

    pub(crate) fn u16_at(&self, at: usize) -> Option<u16> {
        let s = self.bytes.get(at..at.checked_add(2)?)?;
        let v = [s[0], s[1]];
        Some(if self.big_endian {
            u16::from_be_bytes(v)
        } else {
            u16::from_le_bytes(v)
        })
    }

    pub(crate) fn u32_at(&self, at: usize) -> Option<u32> {
        let s = self.bytes.get(at..at.checked_add(4)?)?;
        let v = [s[0], s[1], s[2], s[3]];
        Some(if self.big_endian {
            u32::from_be_bytes(v)
        } else {
            u32::from_le_bytes(v)
        })
    }

    pub(crate) fn u64_at(&self, at: usize) -> Option<u64> {
        let s = self.bytes.get(at..at.checked_add(8)?)?;
        let v = [s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]];
        Some(if self.big_endian {
            u64::from_be_bytes(v)
        } else {
            u64::from_le_bytes(v)
        })
    }

    /// The offset stored at `at`: a u32 in a classic stream, a u64 in a
    /// BigTIFF. `None` when it is unreadable or does not fit a `usize`.
    pub(crate) fn offset_at(&self, at: usize) -> Option<usize> {
        if self.big {
            usize::try_from(self.u64_at(at)?).ok()
        } else {
            usize::try_from(self.u32_at(at)?).ok()
        }
    }

    /// `value` encoded in this stream's byte order.
    pub(crate) fn u16_bytes(&self, value: u16) -> [u8; 2] {
        if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    }

    /// `value` encoded in this stream's byte order.
    pub(crate) fn u32_bytes(&self, value: u32) -> [u8; 4] {
        if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    }

    /// The width of a value field: 4 bytes classic, 8 BigTIFF.
    fn field_width(&self) -> usize {
        if self.big {
            8
        } else {
            4
        }
    }

    /// The entries of the IFD at `at`, plus the offset of its next-IFD link.
    /// Classic: a u16 count, 12-byte entries, a 4-byte link. BigTIFF: a u64
    /// count, 20-byte entries, an 8-byte link. `None` when the count or the
    /// extent runs past the block, or the count is past [`MAX_IFD_ENTRIES`].
    pub(crate) fn ifd(&self, at: usize) -> Option<(Vec<Entry>, usize)> {
        let (count, head, size, value) = if self.big {
            (usize::try_from(self.u64_at(at)?).ok()?, 8, 20, 12)
        } else {
            (usize::from(self.u16_at(at)?), 2, 12, 8)
        };
        if count > MAX_IFD_ENTRIES {
            return None;
        }
        let first = at.checked_add(head)?;
        let link = first.checked_add(count.checked_mul(size)?)?;
        // The link's own bytes must be inside the block too.
        if link.checked_add(self.field_width())? > self.bytes.len() {
            return None;
        }
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let e = first + size * i;
            let entry = Entry {
                tag: self.u16_at(e)?,
                kind: self.u16_at(e + 2)?,
                count: if self.big {
                    self.u64_at(e + 4)?
                } else {
                    u64::from(self.u32_at(e + 4)?)
                },
                field: e + value,
            };
            // An entry whose out-of-line datum leaves the block is corrupt.
            self.data_range(&entry)?;
            entries.push(entry);
        }
        Some((entries, link))
    }

    /// Where the entry's DATA lives: the value field itself when it fits
    /// (4 bytes classic, 8 BigTIFF), the offset it holds otherwise. `None`
    /// for an unreadable extent — which includes an unknown type code, whose
    /// width we refuse to guess.
    pub(crate) fn data_range(&self, e: &Entry) -> Option<std::ops::Range<usize>> {
        let width = self.field_width();
        let Some(unit) = type_size(e.kind, self.big) else {
            // An unknown type is skipped, not fatal: the entry stays opaque.
            return Some(e.field..e.field + width);
        };
        let total = usize::try_from(e.count).ok()?.checked_mul(unit)?;
        if total <= width {
            return Some(e.field..e.field + width);
        }
        let at = self.offset_at(e.field)?;
        let end = at.checked_add(total)?;
        if end > self.bytes.len() {
            return None;
        }
        Some(at..end)
    }

    /// The entry's first value as an unsigned integer, for the inline
    /// SHORT/LONG cases this crate reads.
    pub(crate) fn scalar(&self, e: &Entry) -> Option<u32> {
        match e.kind {
            TYPE_SHORT => Some(u32::from(self.u16_at(e.field)?)),
            TYPE_LONG => Some(self.u32_at(e.field)?),
            _ => None,
        }
    }

    /// A child-IFD pointer: count 1, and a LONG — or, in a BigTIFF only, an
    /// IFD (13), LONG8 (16) or IFD8 (18). A classic stream keeps refusing
    /// type 13, as it always has.
    pub(crate) fn pointer(&self, e: &Entry) -> Option<usize> {
        if e.count != 1 {
            return None;
        }
        match e.kind {
            TYPE_LONG => usize::try_from(self.u32_at(e.field)?).ok(),
            13 if self.big => usize::try_from(self.u32_at(e.field)?).ok(),
            16 | 18 if self.big => usize::try_from(self.u64_at(e.field)?).ok(),
            _ => None,
        }
    }

    /// The entry's first RATIONAL as an f64; `None` for any other shape or a
    /// zero denominator. Read where [`Tiff::data_range`] puts it: out of
    /// line in a classic stream, INLINE for a single RATIONAL in a BigTIFF.
    pub(crate) fn rational(&self, e: &Entry) -> Option<f64> {
        if e.kind != TYPE_RATIONAL || e.count == 0 {
            return None;
        }
        let at = self.data_range(e)?.start;
        let num = f64::from(self.u32_at(at)?);
        let den = f64::from(self.u32_at(at.checked_add(4)?)?);
        if den == 0.0 {
            None
        } else {
            Some(num / den)
        }
    }
}

/// The width of one value of a TIFF field type; `None` for a code this
/// crate does not know. BigTIFF adds IFD (13, 4 bytes) and the 8-byte
/// LONG8 / SLONG8 / IFD8 (16, 17, 18); a classic stream keeps the classic
/// table.
pub(crate) fn type_size(kind: u16, big: bool) -> Option<usize> {
    match kind {
        1 | 2 | 6 | 7 => Some(1),
        3 | 8 => Some(2),
        4 | 9 | 11 => Some(4),
        5 | 10 | 12 => Some(8),
        13 if big => Some(4),
        16..=18 if big => Some(8),
        _ => None,
    }
}

/// The ppi IFD0 states through XResolution / YResolution / ResolutionUnit.
/// Unit 2 is inches and unit 3 centimetres; unit 1 ("none") is a bare
/// aspect ratio and is skipped, as is a malformed directory.
pub(crate) fn resolution_of(t: &Tiff<'_>) -> Option<Resolution> {
    let (entries, _) = t.ifd(t.ifd0)?;
    let find = |tag: u16| entries.iter().find(|e| e.tag == tag);
    let x = t.rational(find(TAG_X_RESOLUTION)?)?;
    let y = t.rational(find(TAG_Y_RESOLUTION)?)?;
    let unit = find(TAG_RESOLUTION_UNIT)
        .and_then(|e| t.scalar(e))
        .unwrap_or(2);
    let scale = match unit {
        2 => 1.0,
        3 => 2.54,
        _ => return None,
    };
    if x <= 0.0 || y <= 0.0 {
        return None;
    }
    Some(Resolution {
        x: (x * scale) as f32,
        y: (y * scale) as f32,
    })
}

/// The ppi an EXIF block states, when the container itself said nothing.
/// Classic only ([`Tiff::new`]): an EXIF blob is never BigTIFF.
pub(crate) fn resolution_from_exif(blob: &[u8]) -> Option<Resolution> {
    resolution_of(&Tiff::new(blob)?)
}

const TAG_MAKE: u16 = 0x010F;
const TAG_MODEL: u16 = 0x0110;
const TAG_F_NUMBER: u16 = 0x829D;
const TAG_SUBJECT_DISTANCE: u16 = 0x9206;
const TAG_FOCAL_LENGTH: u16 = 0x920A;
const TAG_FOCAL_LENGTH_35MM: u16 = 0xA405;
const TAG_LENS_MAKE: u16 = 0xA433;
const TAG_LENS_MODEL: u16 = 0xA434;
const TYPE_ASCII: u16 = 2;

/// The camera and lens an EXIF block names, for Lensfun matching
/// (v1-lensfun; `lens_db`): IFD0's Make and Model, and the Exif IFD's
/// LensMake, LensModel, FocalLength, FNumber, SubjectDistance,
/// FocalLengthIn35mmFilm and PixelX/YDimension (SHORT or LONG). The third
/// narrow place the core reads inside EXIF (`metadata`'s module doc). An
/// unreadable block or field is unknown, never an error; a zero is "not
/// recorded".
pub(crate) fn lens_identity_from_exif(blob: &[u8]) -> crate::lens_db::LensIdentity {
    let mut identity = crate::lens_db::LensIdentity::default();
    let Some(t) = Tiff::new(blob) else {
        return identity;
    };
    let Some((ifd0, _)) = t.ifd(t.ifd0) else {
        return identity;
    };
    let text = |entries: &[Entry], tag: u16| -> Option<String> {
        let e = entries
            .iter()
            .find(|e| e.tag == tag && e.kind == TYPE_ASCII)?;
        let bytes = t.bytes.get(t.data_range(e)?)?;
        let count = usize::try_from(e.count).ok()?.min(bytes.len());
        let bytes = &bytes[..count];
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        let s = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
        (!s.is_empty()).then_some(s)
    };
    let positive = |v: Option<f64>| v.filter(|v| v.is_finite() && *v > 0.0);
    identity.make = text(&ifd0, TAG_MAKE);
    identity.model = text(&ifd0, TAG_MODEL);
    let exif = ifd0
        .iter()
        .find(|e| e.tag == TAG_EXIF_IFD)
        .and_then(|e| t.pointer(e))
        .and_then(|at| t.ifd(at));
    let Some((exif, _)) = exif else {
        return identity;
    };
    let find = |tag: u16| exif.iter().find(|e| e.tag == tag);
    let rational = |tag: u16| positive(find(tag).and_then(|e| t.rational(e)));
    let integer = |tag: u16| {
        find(tag)
            .filter(|e| e.count >= 1)
            .and_then(|e| t.scalar(e))
            .filter(|v| *v > 0)
    };
    identity.lens_make = text(&exif, TAG_LENS_MAKE);
    identity.lens_model = text(&exif, TAG_LENS_MODEL);
    identity.focal_length = rational(TAG_FOCAL_LENGTH);
    identity.f_number = rational(TAG_F_NUMBER);
    identity.subject_distance = rational(TAG_SUBJECT_DISTANCE);
    identity.focal_length_35mm = integer(TAG_FOCAL_LENGTH_35MM).map(f64::from);
    identity.pixel_width = integer(TAG_PIXEL_X_DIMENSION);
    identity.pixel_height = integer(TAG_PIXEL_Y_DIMENSION);
    identity
}
