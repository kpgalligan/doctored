//! The field widths and row framing the three layered containers share, with
//! the ONE PackBits decoder. A leaf: it imports nothing from the crate.
//!
//! A layered picture travels in one of three containers
//! ([`LayeredContainer`]): the classic PSD (version 1), the large-document PSB
//! (version 2) and a TIFF whose tag 37724 holds Photoshop's layer block. They
//! hold the SAME records; only the widths differ, and Adobe's specification
//! (https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/) states each:
//!
//! | field | PSD, TIFF block | PSB |
//! |---|---|---|
//! | layer-and-mask section length, layer-info length | u32 | u64 |
//! | a record's channel data length | u32 | u64 |
//! | an RLE plane's per-row byte count | u16 | u32 |
//! | a tagged block's length | u32 | u32, but u64 for [`LONG_LENGTH_KEYS`] |
//! | side limit | 30 000 | 300 000 |
//!
//! [`rle_rows`] is the one walk of an RLE plane's row table at either width:
//! every reader (the stored composite, layer channels, masks, at both depths)
//! takes its rows from it, so a row can never be framed two ways. It returns
//! borrowed rows and decodes nothing; [`validate_packbits`] stays the MASTER
//! copy of the control-byte interpretation and [`unpack_packbits`] is its
//! decode twin, the only PackBits decoder in the crate.

/// Which file a layered export writes, or a layered import read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LayeredContainer {
    Psd = 0,
    Psb = 1,
    Tiff = 2,
}

/// The refusal for a container code outside the three.
pub(crate) const UNKNOWN_CONTAINER: &str =
    "unknown layered container; use 0 (PSD), 1 (PSB) or 2 (TIFF)";

impl LayeredContainer {
    pub(crate) fn from_c(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Psd),
            1 => Some(Self::Psb),
            2 => Some(Self::Tiff),
            _ => None,
        }
    }

    /// The name reports and sentences use.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Psd => "PSD",
            Self::Psb => "PSB",
            Self::Tiff => "TIFF",
        }
    }

    /// True for the one container with 64-bit section and channel lengths and
    /// 32-bit RLE row counts. A TIFF's layer block is framed like a classic
    /// PSD's.
    pub(crate) fn large(self) -> bool {
        self == Self::Psb
    }

    /// The format's own per-side pixel limit. Adobe gives 30 000 for PSD and
    /// 300 000 for PSB; a layered TIFF is held to the classic limit because
    /// its block uses the classic 16-bit row counts.
    pub(crate) fn max_side(self) -> u32 {
        match self {
            Self::Psd | Self::Tiff => 30_000,
            Self::Psb => 300_000,
        }
    }

    /// The largest file the container can frame: a classic PSD's unsigned
    /// 32-bit sections, which Photoshop refuses at or above 2 GiB; a classic
    /// TIFF's 32-bit offsets; PSB has no limit this crate can reach.
    pub(crate) fn max_file_bytes(self) -> u64 {
        match self {
            Self::Psd => 2_147_483_647,
            Self::Psb => u64::MAX,
            Self::Tiff => 4_294_967_295,
        }
    }
}

/// Adobe's channel-count cap: a PSD or PSB header declares 1 to 56 channels,
/// the picture's own planes (and merged alpha) plus every saved channel. The
/// reader refuses a header past it and the writer never declares more
/// (`psd_export_preflight::spots_capacity`).
pub(crate) const MAX_PSD_CHANNELS: usize = 56;

/// The tagged-block keys whose length is 8 bytes in a PSB (Adobe's
/// "Additional Layer Information" note), every other key keeping 4.
pub(crate) const LONG_LENGTH_KEYS: [[u8; 4]; 14] = [
    *b"LMsk", *b"Lr16", *b"Lr32", *b"Layr", *b"Mt16", *b"Mt32", *b"Mtrn", *b"Alph", *b"FMsk",
    *b"lnk2", *b"FEid", *b"FXid", *b"PxSD", *b"cinf",
];

/// The rows of one RLE plane, borrowed and undecoded. `plane` is the bytes
/// AFTER the compression word: `rows` byte counts (u16, or u32 when `large`)
/// and then the rows themselves, which must end exactly where the plane does.
/// A truncated table or row, or bytes after the last row, is `Err`.
pub(crate) fn rle_rows(plane: &[u8], rows: usize, large: bool) -> Result<Vec<&[u8]>, String> {
    let width = if large { 4 } else { 2 };
    let table_end = rows.checked_mul(width).ok_or("PSD RLE size overflow")?;
    let table = plane
        .get(..table_end)
        .ok_or("truncated PSD RLE row table")?;
    let mut out = Vec::with_capacity(rows);
    let mut offset = table_end;
    for count in table.chunks_exact(width) {
        let count = if large {
            u32::from_be_bytes([count[0], count[1], count[2], count[3]]) as usize
        } else {
            usize::from(u16::from_be_bytes([count[0], count[1]]))
        };
        let end = offset.checked_add(count).ok_or("PSD RLE size overflow")?;
        out.push(plane.get(offset..end).ok_or("truncated PSD RLE row")?);
        offset = end;
    }
    if offset != plane.len() {
        return Err("PSD plane has trailing RLE data".into());
    }
    Ok(out)
}

/// The decode twin of [`validate_packbits`], which stays the MASTER copy of
/// the control-byte interpretation: identical run/literal/no-op handling, with
/// an output buffer. It is the ONE in-crate PackBits decoder, used by every
/// plane at both depths. `row_bytes` is the decoded row length in BYTES.
/// `pub` only so `psd_container` can keep re-exporting it for the black-box
/// tests that decode a written plane with it.
pub fn unpack_packbits(bytes: &[u8], row_bytes: usize) -> Result<Vec<u8>, String> {
    validate_packbits(bytes, row_bytes)?;
    let mut out = Vec::with_capacity(row_bytes);
    let mut cursor = 0;
    while cursor < bytes.len() {
        let control = bytes[cursor] as i8;
        cursor += 1;
        match control {
            0..=127 => {
                let count = control as usize + 1;
                out.extend_from_slice(&bytes[cursor..cursor + count]);
                cursor += count;
            }
            -127..=-1 => {
                let count = (1i16 - i16::from(control)) as usize;
                out.extend(std::iter::repeat_n(bytes[cursor], count));
                cursor += 1;
            }
            -128 => {}
        }
    }
    Ok(out)
}

/// Validate one PackBits row's decoded length. This is framing, not a second
/// decompressor: no output buffer is produced and no samples are interpreted.
pub(crate) fn validate_packbits(bytes: &[u8], width: usize) -> Result<(), String> {
    let (mut cursor, mut output) = (0, 0usize);
    while cursor < bytes.len() {
        let control = bytes[cursor] as i8;
        cursor += 1;
        let (input, count) = match control {
            0..=127 => (control as usize + 1, control as usize + 1),
            -127..=-1 => (1, (1i16 - i16::from(control)) as usize),
            -128 => (0, 0),
        };
        cursor = cursor.checked_add(input).ok_or("PSD RLE row overflow")?;
        output = output.checked_add(count).ok_or("PSD RLE row overflow")?;
        if cursor > bytes.len() || output > width {
            return Err("invalid PSD RLE row".into());
        }
    }
    if output != width {
        return Err("PSD RLE row has incorrect decoded width".into());
    }
    Ok(())
}
