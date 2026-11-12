//! The ONE codec for Photoshop's `lspf` (layer protection) block, both
//! directions and both depths.
//!
//! Adobe's additional-layer-information table
//! (https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/) gives
//! `lspf` as a 4-byte protection-flags word: bit 0 transparency, bit 1
//! composite (pixels), bit 2 position. psd-tools' `ProtectedSetting`
//! (psd_tools/constants.py) names the same bits 1/2/4 plus 8 (nesting, the
//! artboard lock) and `complete = 0x80000000`, Photoshop's "Lock All"; ag-psd
//! (src/additionalInfo.ts, `lspf`) reads bits 0–3 the same way. Anything else
//! in the word is not a documented lock, so [`word`] refuses it whole rather
//! than guessing.
//!
//! Evidence from Photoshop's own files: record 0 of
//! `tests/fixtures/psd_photoshop/knockout/source.psd` and `source16.psd` — the
//! Background — carries `lspf 0x0d` (transparency | position | nesting) with
//! header flags `0x09`, and every other record, the section dividers
//! included, carries `lspf 0`. Those are the Background's IMPLICIT locks:
//! Doctored's Background mark (`doc_background`) locks nothing, so
//! [`background`] masks them off and keeps only what the user could have
//! added (pixels), and [`background_body`] puts them back on a written
//! Background record (`psd_background`).
//! ImageMagick's `samples/layered.psd` sets header flag bit 0 with no `lspf`
//! at all, so header bit 0 is never read as a lock here.
//!
//! The Lock All mapping is lossy in both directions and stated: Photoshop's
//! complete lock also covers opacity, blend mode and layer style, and
//! Doctored's `LOCK_ALL` does not. A file's separate `0x7` imports as
//! `LOCK_ALL` too — which in Doctored also freezes the mask (`doc_lock`) —
//! and is written back as `0x80000000`. The artboard-nesting bit is dropped.
//!
//! `psd_locks.rs` is the ONE `lspf` codec, both directions and both depths: a word
//! holding only Photoshop's documented bits (0–3 and Lock All's bit 31) carries
//! transparency/pixels/position/Lock All onto the layer or kept folder (the nesting bit dropped, a
//! Background's implicit `0x0d` masked off, header flag bit 0 never read as a lock, a folder's lock
//! on the group, which covers its contents; a folder dissolved past the nesting cap ORs its
//! bits into each layer it held), anything else is a PROTECTION loss.

use crate::doc_lock::LOCK_ALL;

/// psd-tools' `ProtectedSetting.complete`: Photoshop's "Lock All".
pub(crate) const LOCK_ALL_WORD: u32 = 0x8000_0000;

/// Bits Photoshop documents: Lock All plus the four individual locks.
const KNOWN_BITS: u32 = 0x8000_000F;

/// The implicit locks of a Photoshop Background: transparency, position and
/// nesting (`0x0d`, measured above).
const BACKGROUND_IMPLICIT: u32 = 0x0D;

/// The big-endian protection word of an `lspf` body, or `None` when the body
/// is not exactly four bytes or sets a bit no source documents.
pub(crate) fn word(body: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = body.try_into().ok()?;
    let word = u32::from_be_bytes(bytes);
    (word & !KNOWN_BITS == 0).then_some(word)
}

/// Doctored's lock bits for an admitted word: Lock All (bit 31) is
/// `LOCK_ALL`; otherwise bits 0..2 carry directly (`doc_lock` shares their
/// positions) and the nesting bit (`0x8`) is dropped.
pub(crate) fn locks(word: u32) -> u32 {
    if word & LOCK_ALL_WORD != 0 {
        LOCK_ALL
    } else {
        word & LOCK_ALL
    }
}

/// A Background record's word with its implicit transparency, position and
/// nesting locks removed. An explicit Lock All is the user's and is kept.
pub(crate) fn background(word: u32) -> u32 {
    if word & LOCK_ALL_WORD != 0 {
        word
    } else {
        word & !BACKGROUND_IMPLICIT
    }
}

/// The `lspf` body for Doctored lock bits (reserved bits masked off), or
/// `None` for an unlocked layer, so an unlocked document writes no block.
pub(crate) fn encode(locks: u32) -> Option<[u8; 4]> {
    let locks = locks & LOCK_ALL;
    let word = match locks {
        0 => return None,
        LOCK_ALL => LOCK_ALL_WORD,
        bits => bits,
    };
    Some(word.to_be_bytes())
}

/// The writers' one call: the `lspf` block for a record, if it is locked.
pub(crate) fn block(locks: u32) -> Option<([u8; 4], Vec<u8>)> {
    Some((*b"lspf", encode(locks)?.to_vec()))
}

/// A Background record's `lspf` body (`psd_background`): the layer's own
/// bits with the Background's implicit `0x0d` OR'd in, Photoshop's form; an
/// explicit Lock All stays the complete lock, which [`background`] keeps on
/// the way back in.
pub(crate) fn background_body(locks: u32) -> [u8; 4] {
    let word = match encode(locks) {
        Some(bytes) if u32::from_be_bytes(bytes) == LOCK_ALL_WORD => LOCK_ALL_WORD,
        Some(bytes) => u32::from_be_bytes(bytes) | BACKGROUND_IMPLICIT,
        None => BACKGROUND_IMPLICIT,
    };
    word.to_be_bytes()
}
