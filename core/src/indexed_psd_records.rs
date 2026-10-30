//! KERNEL: the ONE reader and writer of an Indexed Color PSD's colour table
//! (#1): the 768-byte Color Mode Data section and image resources 1046
//! (the colour count) and 1047 (the transparent index). `indexed_psd` reads
//! and writes the file around them.
//!
//! Adobe's Photoshop File Format Specification
//! (https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/): an
//! Indexed Color file's Color Mode Data section is 768 bytes, the 256 reds,
//! then the 256 greens, then the 256 blues (planar, not interleaved).
//! Resource 1046 is a u16 count of the entries in use; a file without it
//! uses all 256. Resource 1047 is a u16 index whose entry is transparent.
//!
//! A count outside 1..=256 is read as 256, because the table always holds
//! 256 entries and a count past it names nothing; a 1047 at or past the
//! count names no entry in use, so it is no transparent index. The writer
//! pads the table past the count with black (zeros) and writes 1047 as
//! 0xFFFF when there is no transparent index, which is what Photoshop's own
//! files hold (`core/tests/fixtures/psd_photoshop/indexed_547`, take1).

use crate::indexed_palette::Rgb;

/// The Color Mode Data section's length in an Indexed Color file.
pub(crate) const COLOR_MODE_DATA_BYTES: usize = 768;

/// The image resource that holds the count of entries in use.
pub(crate) const TABLE_COUNT: u16 = 1046;

/// The image resource that holds the transparent index.
pub(crate) const TRANSPARENT_INDEX: u16 = 1047;

/// The table entries a file holds, used or not.
const ENTRIES: usize = 256;

/// A file's colour table: the entries in use and the transparent index.
pub(crate) struct IndexedTable {
    pub palette: Vec<Rgb>,
    pub transparent: Option<u8>,
}

/// A resource body's leading u16, when it has one.
fn word(body: &[u8]) -> Option<u16> {
    match body.get(..2) {
        Some(&[a, b]) => Some(u16::from_be_bytes([a, b])),
        _ => None,
    }
}

/// The table of an Indexed Color file (module doc): `mode_data` is the
/// Color Mode Data section, `count` resource 1046's body and `transparent`
/// resource 1047's, each `None` when the file has none. Refuses a section
/// that is not 768 bytes.
pub(crate) fn read_table(
    mode_data: &[u8],
    count: Option<&[u8]>,
    transparent: Option<&[u8]>,
) -> Result<IndexedTable, String> {
    if mode_data.len() != COLOR_MODE_DATA_BYTES {
        return Err(format!(
            "an Indexed Color PSD's colour table is {COLOR_MODE_DATA_BYTES} bytes; this file's is {}",
            mode_data.len()
        ));
    }
    let used = count
        .and_then(word)
        .map(usize::from)
        .filter(|n| (1..=ENTRIES).contains(n))
        .unwrap_or(ENTRIES);
    let palette = (0..used)
        .map(|i| {
            [
                mode_data[i],
                mode_data[ENTRIES + i],
                mode_data[2 * ENTRIES + i],
            ]
        })
        .collect();
    let transparent = transparent
        .and_then(word)
        .filter(|&t| usize::from(t) < used)
        .map(|t| t as u8);
    Ok(IndexedTable {
        palette,
        transparent,
    })
}

/// The 768-byte Color Mode Data section of `palette` (at most 256
/// entries): planar R, G, B, each padded with zeros past the palette.
pub(crate) fn color_mode_data(palette: &[Rgb]) -> Vec<u8> {
    let mut out = vec![0u8; COLOR_MODE_DATA_BYTES];
    for (i, rgb) in palette.iter().take(ENTRIES).enumerate() {
        for (plane, &value) in rgb.iter().enumerate() {
            out[plane * ENTRIES + i] = value;
        }
    }
    out
}

/// Resource 1046's body for a palette of `len` entries.
pub(crate) fn count_body(len: usize) -> [u8; 2] {
    (len.min(ENTRIES) as u16).to_be_bytes()
}

/// Resource 1047's body: the transparent index, or with none 0xFFFF, which
/// names no entry (Photoshop writes 1047 in every Indexed Color file, 0xFFFF
/// when the image has no transparent index: the `indexed_547` fixtures).
pub(crate) fn transparent_body(index: Option<u8>) -> [u8; 2] {
    index.map_or(u16::MAX, u16::from).to_be_bytes()
}
