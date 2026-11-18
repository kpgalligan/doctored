//! A PSD path's per-subpath OPERATION WORDS (KERNEL): the word each length
//! record names (`psd_path_records`), read into `path_geometry::SubpathOp`
//! ([`op_from_word`]) and written back from it ([`word_of`]). 1 unites
//! (combine), 2 subtracts, 3 intersects and 0 excludes (xor). The subpaths
//! fold in file order from no pixels, or from all of them when the initial
//! fill record says so or the FIRST subpath subtracts (Photoshop's 8-bit
//! rendering of such a mask is everything outside the subpath, the
//! `authored_exchange` family's `Ve`); a vector mask whose flags say
//! inverted is inverted last. The bake of a mask Doctored does not carry
//! draws that fold as one coverage of the shape (`psd_vector_mask_bake`).

use crate::path_geometry::SubpathOp;

/// The operation a length record's operation word names
/// (`psd_path_records`), `None` for an unknown one.
pub(crate) fn op_from_word(word: i16) -> Option<SubpathOp> {
    match word {
        0 => Some(SubpathOp::Exclude),
        1 => Some(SubpathOp::Unite),
        2 => Some(SubpathOp::Subtract),
        3 => Some(SubpathOp::Intersect),
        _ => None,
    }
}

/// [`op_from_word`]'s inverse: the length record's word for `op`.
pub(crate) fn word_of(op: SubpathOp) -> i16 {
    match op {
        SubpathOp::Exclude => 0,
        SubpathOp::Unite => 1,
        SubpathOp::Subtract => 2,
        SubpathOp::Intersect => 3,
    }
}
