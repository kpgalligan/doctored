//! Whether a TIFF carries Photoshop's layer block WITH LAYERS in it (tag
//! 37724, `tiff_layers`' module doc), whatever the file's byte order, its
//! form (classic or BigTIFF), colour model, depth and sample format.
//!
//! [`has_layer_block`] is the question File ▸ Save asks of a TIFF a document
//! was opened FLAT from (`dr_tiff_has_layer_block`). `tiff_layers::has_layers`
//! answers only for the files the layered open reads; every other file with
//! the tag (little-endian at 8 or 16 bits, a 32-bit file that is not float
//! RGB, Palette, a 16-bit float, BigTIFF) opens flat, as does a readable one
//! opened with Open Flattened or by an agent without `psd_mode`. Writing the
//! flat image over such a file would erase layers the document does not hold.
//!
//! **A block holds layers** when one of its `Layr`, `Lr16` or `Lr32` entries
//! has a body whose first two bytes, the i16 record count, are not both zero.
//! Zero is the same two bytes in either byte order, so the answer does not
//! depend on how a little-endian file at 8 or 16 bits lays out its records,
//! which no authored file confirms. Each entry's signature says its order
//! (`8BIM` big-endian, `MIB8` little-endian: [`ByteOrder::code`]), and its
//! key and length are read in that order.
//!
//! **Unknown answers true.** A value that begins with the block's prefix but
//! whose entries cannot be walked (another signature, a length past the value
//! or the file, more than [`MAX_BLOCKS`] entries) holds content this crate
//! cannot see, and the answer that keeps the file is true. An unreadable
//! file, a non-TIFF, no tag 37724, a value without the prefix and a walk that
//! ends with no layer entry holding records answer false.
//!
//! **What is read**: the header, the first directory, the entry heads and two
//! bytes of each layer entry's body; never a pixel strip. A classic
//! directory is `tiff_directory::Directory::first`'s. The BigTIFF header and
//! directory are read here (version 43, offset size 8, reserved 0, a u64
//! directory offset; a u64 count of 20-byte entries, each a u16 tag and type,
//! a u64 count and an 8-byte value field), the layout `tiff_view::Tiff::ifd`
//! reads over a whole file in memory. The entry walk is `tiff_layers::scan`'s
//! (12-byte heads, bodies padded to 4, fewer than 12 bytes left is the end).

use crate::tiff_directory::{file_reader, Directory, MAX_ENTRIES};
use crate::tiff_layers::{ByteOrder, BLOCK_HEADER, MAX_BLOCKS, TAG_LAYERS};

/// `read(offset, length)`: exactly `length` bytes of the file, or `None`.
type Reader<'a> = &'a mut dyn FnMut(u64, usize) -> Option<Vec<u8>>;

/// The layer-list keys, one per depth, in their big-endian spelling.
const LAYER_KEYS: [[u8; 4]; 3] = [*b"Layr", *b"Lr16", *b"Lr32"];

/// Whether the file at `path` carries a layer block with layers in it, or
/// one whose entries cannot be walked (module doc). False for an unreadable
/// path.
pub(crate) fn has_layer_block(path: &str) -> bool {
    let Ok(mut read) = file_reader(path) else {
        return false;
    };
    let Some((start, length)) = block_value(&mut read) else {
        return false;
    };
    holds_layers(&mut read, start, length)
}

/// Tag 37724's value as (offset, length), from the first directory of a
/// classic TIFF or a BigTIFF, when it is byte-typed (BYTE 1 or UNDEFINED 7)
/// and out of line: a value that fits the entry's own field cannot hold the
/// block's 36-byte prefix.
fn block_value(read: Reader) -> Option<(u64, u64)> {
    let head = read(0, 8)?;
    let order = match &head[..2] {
        b"MM" => ByteOrder::Big,
        b"II" => ByteOrder::Little,
        _ => return None,
    };
    match order.u16(&head[2..]) {
        42 => {
            let directory = Directory::first(read)?;
            let entry = directory.entry(TAG_LAYERS)?;
            (matches!(entry.kind, 1 | 7) && entry.count > 4)
                .then(|| (directory.offset_of(entry), u64::from(entry.count)))
        }
        43 => big_tiff_value(read, order, &head),
        _ => None,
    }
}

/// [`block_value`] for a BigTIFF (module doc), whose `head` is already read.
fn big_tiff_value(read: Reader, order: ByteOrder, head: &[u8]) -> Option<(u64, u64)> {
    if order.u16(&head[4..]) != 8 || order.u16(&head[6..]) != 0 {
        return None;
    }
    let directory = order.u64(&read(8, 8)?);
    let count = usize::try_from(order.u64(&read(directory, 8)?)).ok()?;
    if count > MAX_ENTRIES {
        return None;
    }
    let entries = read(directory.checked_add(8)?, count * 20)?;
    let entry = entries
        .chunks_exact(20)
        .find(|entry| order.u16(entry) == TAG_LAYERS)?;
    let (kind, length) = (order.u16(&entry[2..]), order.u64(&entry[4..]));
    (matches!(kind, 1 | 7) && length > 8).then(|| (order.u64(&entry[12..]), length))
}

/// Whether the value at `start` of `length` bytes is the block's prefix
/// followed by entries that hold layers or cannot be walked (module doc).
fn holds_layers(read: Reader, start: u64, length: u64) -> bool {
    let prefix = BLOCK_HEADER.len();
    if length < prefix as u64 || read(start, prefix).as_deref() != Some(BLOCK_HEADER) {
        return false;
    }
    // Past the prefix, a walk that fails is content this crate cannot see.
    let Some(end) = start.checked_add(length) else {
        return true;
    };
    walk(read, start + prefix as u64, end).unwrap_or(true)
}

/// `Some(true)` at the first layer entry whose record count is not zero,
/// `Some(false)` when the entries end with none, `None` when they cannot be
/// walked.
fn walk(read: Reader, mut cursor: u64, end: u64) -> Option<bool> {
    for _ in 0..MAX_BLOCKS {
        if cursor.checked_add(12)? > end {
            return Some(false);
        }
        let head = read(cursor, 12)?;
        let order = match &head[..4] {
            b"8BIM" => ByteOrder::Big,
            b"MIB8" => ByteOrder::Little,
            _ => return None,
        };
        let body = u64::from(order.u32(&head[8..]));
        let body_start = cursor + 12;
        let body_end = body_start.checked_add(body)?;
        if body_end > end {
            return None;
        }
        let key = [head[4], head[5], head[6], head[7]];
        if body >= 2
            && LAYER_KEYS.iter().any(|layers| order.code(*layers) == key)
            && read(body_start, 2)? != [0, 0]
        {
            return Some(true);
        }
        cursor = body_end.checked_add(body.wrapping_neg() & 3)?;
    }
    None
}
