//! Exact EXIF orientation permutation, shared by integer decoder intake paths.
//!
//! The existing preserving PNG/TIFF intake shares the orientation mapping.
pub(crate) fn dimensions(w: usize, h: usize, orientation: u8) -> (usize, usize) {
    if (5..=8).contains(&orientation) {
        (h, w)
    } else {
        (w, h)
    }
}
pub(crate) fn source(x: usize, y: usize, w: usize, h: usize, orientation: u8) -> (usize, usize) {
    match orientation {
        2 => (w - 1 - x, y),
        3 => (w - 1 - x, h - 1 - y),
        4 => (x, h - 1 - y),
        5 => (y, x),
        6 => (y, h - 1 - x),
        7 => (w - 1 - y, h - 1 - x),
        8 => (w - 1 - y, x),
        _ => (x, y),
    }
}
