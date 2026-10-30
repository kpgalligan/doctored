//! The Lab Color mode's own memory limits (#1 Lab phase 3b): a leaf that
//! imports nothing from the crate (core/CLAUDE.md, "Layers").

/// Bytes a canvas pixel the flat Lab TIFF open charges beside the encoded
/// file, the bookkeeping and the wider decoded samples
/// (`lab_tiff_read::open_scratch`): the RGBA16 tiles (8 B/px) and the opened
/// document's display preparation the host runs right after — the Display
/// P3 view of the codes and its narrowing — with the margin.
///
/// Phase 3a charged Gray's `gray_io::OPEN_BYTES_PER_PIXEL` (41) here, and
/// `scripts/lab-admission/measure.py` measured the open's peak at about 2.02×
/// below that plan (`open-lab-tiff16` and `open-lab-tiff8` at 24 MP, about
/// 24.2 B/px with the file). 24 B/px, plus the open's `extra` (2 B/px at 16
/// bits, 5 at 8) and the file (6 or 3 B/px), holds every open to ⌈1.25 ×
/// its measured peak⌉, which the same two cases check; a miss raises this
/// term.
pub(crate) const OPEN_BYTES_PER_PIXEL: u64 = 24;
