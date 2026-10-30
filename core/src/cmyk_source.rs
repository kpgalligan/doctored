//! Which files open as CMYK documents (#1 phase 1), header work only.
//!
//! An 8/16-bit CMYK TIFF (chunky or planar, four samples or more: the first
//! extra is its transparency, phase 3) and a four-component CMYK/YCCK JPEG
//! that pass `open_profile::cmyk_gate` (the flat open's own gate) and have a
//! CMYK profile, their own or the working one (`cmyk_io::file_policy`). A
//! PSD/PSB in colour mode 4, and a layered CMYK TIFF opened with Import
//! Layers, are the PSD importer's question (`import_precision` "cmyk"), not
//! this one's. With no
//! working profile set an untagged TIFF or JPEG answers `None`, so
//! `open_preserving` and the host keep today's path (the RGB conversion
//! under the untagged-CMYK sentence). [`cmyk_source`] asks the bytes an open
//! already holds; [`cmyk_source_at`] asks a path without reading the file whole (a
//! JPEG's header segments, a TIFF's directory through a file-backed
//! decoder), which is what the host's open routing calls.

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};

use tiff::decoder::Decoder;

use crate::cmyk_document::CmykDepth;
use crate::cmyk_io::file_policy;
use crate::open_profile::{cmyk_container, cmyk_gate};

/// `DR_CMYK_SOURCE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmykSource {
    Tiff = 1,
    Jpeg = 2,
}

/// Which CMYK container `bytes` is, header work only (module doc); `None`
/// for every file that does not open as a CMYK document.
pub(crate) fn cmyk_source(bytes: &[u8]) -> Option<CmykSource> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return jpeg_source(bytes);
    }
    tiff_source(Cursor::new(bytes))
}

/// [`cmyk_source`]'s JPEG arm, on the whole file or on its header segments
/// alone ([`jpeg_header`]): every byte it reads is before the first SOS.
fn jpeg_source(bytes: &[u8]) -> Option<CmykSource> {
    let container = cmyk_container(bytes)?;
    cmyk_gate(bytes, "the file", container).ok()?;
    let icc = crate::dr_image::embedded_icc(bytes);
    file_policy(icc.as_deref(), CmykDepth::Eight)?;
    Some(CmykSource::Jpeg)
}

/// [`cmyk_source`]'s TIFF arm over any seekable reader: the gate's layout
/// (`open_profile::cmyk_tiff_layout`, which also refuses every non-CMYK
/// colour type) and IFD0's ICC profile, read under the metadata walk's own
/// packet cap (`MAX_DRDC_BLOB_LEN`), so a file and its bytes answer alike.
fn tiff_source<R: Read + Seek>(reader: R) -> Option<CmykSource> {
    let mut decoder = Decoder::new(reader).ok()?;
    crate::open_profile::cmyk_tiff_layout(&mut decoder, "the file").ok()?;
    let mut limits = tiff::decoder::Limits::default();
    limits.ifd_value_size = crate::drdc::MAX_DRDC_BLOB_LEN as usize;
    let mut decoder = decoder.with_limits(limits);
    let icc = decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).ok();
    file_policy(icc.as_deref(), CmykDepth::Eight)?;
    Some(CmykSource::Tiff)
}

/// The most header bytes [`jpeg_header`] collects before it gives up: 255
/// APP2 chunks carry a ~16 MiB ICC profile, and EXIF and XMP add a few
/// segments more, so a real file stays far below it while a malformed one
/// cannot make the header question read the file whole.
const JPEG_HEADER_CAP: usize = 32 << 20;

/// A JPEG's bytes from SOI through its first SOS segment — everything
/// [`jpeg_source`] reads — without the entropy-coded data after it. `None`
/// for a malformed marker run, a short file or a header over
/// [`JPEG_HEADER_CAP`].
pub(crate) fn jpeg_header(reader: &mut impl Read) -> Option<Vec<u8>> {
    let mut out = vec![0xFF, 0xD8];
    let mut soi = [0u8; 2];
    reader.read_exact(&mut soi).ok()?;
    (soi == [0xFF, 0xD8]).then_some(())?;
    loop {
        let mut marker = [0u8; 2];
        reader.read_exact(&mut marker).ok()?;
        (marker[0] == 0xFF).then_some(())?;
        // Fill bytes: any number of 0xFF may precede a marker code.
        while marker[1] == 0xFF {
            reader.read_exact(&mut marker[1..]).ok()?;
        }
        out.extend_from_slice(&marker);
        // TEM and RSTn stand alone; EOI ends a header-only file.
        match marker[1] {
            0x01 | 0xD0..=0xD7 => continue,
            0xD9 => return Some(out),
            _ => {}
        }
        let mut field = [0u8; 2];
        reader.read_exact(&mut field).ok()?;
        let length = usize::from(u16::from_be_bytes(field));
        (length >= 2 && out.len() + length <= JPEG_HEADER_CAP).then_some(())?;
        out.extend_from_slice(&field);
        let at = out.len();
        out.resize(at + length - 2, 0);
        reader.read_exact(&mut out[at..]).ok()?;
        if marker[1] == 0xDA {
            return Some(out);
        }
    }
}

/// [`cmyk_source`] for the file at `path`, header work only: the JPEG's
/// header segments ([`jpeg_header`]) or a TIFF's directory through a
/// file-backed decoder; `None` for every other file. `Err` only when the
/// file cannot be opened.
pub(crate) fn cmyk_source_at(path: &str) -> Result<Option<CmykSource>, String> {
    let mut file = File::open(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("failed to read {path}: {e}"))?;
    if magic.starts_with(&[0xFF, 0xD8]) {
        return Ok(jpeg_header(&mut BufReader::new(file)).and_then(|h| jpeg_source(&h)));
    }
    if !crate::metadata_tiff::is_tiff(&magic) {
        return Ok(None);
    }
    Ok(tiff_source(BufReader::new(file)))
}
