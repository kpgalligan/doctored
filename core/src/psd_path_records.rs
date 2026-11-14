//! Adobe's PATH RECORDS (KERNEL): the 26-byte records a saved path's image
//! resource (2000–2997), the work path's (1025), the clipping path's
//! reference (2999) and a layer's vector mask (`vmsk`) are made of, both
//! directions, over `path_geometry::PathShape`. `psd_paths` is the adapter.
//!
//! Each record starts with a u16 selector: 0 / 3 a closed / open subpath's
//! length record (u16 knot count, i16 combine operation, u16 1, then zeros),
//! 1 / 2 a closed subpath's linked / unlinked Bézier knot, 4 / 5 the open
//! subpath's, 6 the path fill rule record, 7 the clipboard record, 8 the
//! initial fill rule record (u16 1: the path starts from all pixels). A knot
//! is three points — the control point that PRECEDES the anchor, the anchor,
//! the one that LEAVES it — each a vertical then a horizontal signed 8.24
//! fixed-point FRACTION of the canvas height and width. Doctored's anchor is
//! `(handle_in, point, handle_out)` in canvas pixels, `smooth` ↔ linked.
//! Evidence: the `vmsk` of `cmyk-psd-547/take1/unsupported/unsupported8.psd`'s
//! "Shape" record (`tests/psd_paths.rs` carries it).
//!
//! **Resolution.** 8.24 holds a fraction to 2⁻²⁴, so a coordinate crosses
//! within `canvas side / 2²⁴` px (under 0.02 px at the 300,000 px wall) and
//! Doctored's stored four decimals; a coordinate past ±128 canvases does not
//! fit and the path is not written ([`encode`]'s `Err`).
//!
//! **The clipping path (2999).** Its body is a Pascal string, the name of
//! the saved path it designates, then, with NO pad after the name, the
//! flatness as a 16.16 fixed-point number of device pixels (0 when none is
//! set) and a u16 1: the five files of the `authored_exchange` family, authored
//! by Photoshop 26.10.0 with names of 4 and 5 bytes and flatness none, 0.5
//! and 3, PSD and flat TIFF alike. [`decode_clipping`] hands the bytes after
//! the name back as the tail, which the caller keeps verbatim (no pad means
//! a rename never moves it) but for the no-flatness tail, kept empty as a
//! mark made in Doctored is ([`kept_clipping_tail`]); [`encode_clipping`]
//! writes a kept tail back verbatim and an empty one (also what a build that
//! wrote the name alone kept) as the authored no-flatness tail.
//!
//! **Fill rule.** Photoshop combines a path's subpaths by an operation per
//! subpath; Doctored fills a path by one rule. Every subpath `0` (exclude
//! overlapping shapes) reads as `evenodd`, every subpath `1` (combine) as
//! `nonzero`, which agree wherever contours do not cancel by winding; any
//! other mix reads as `nonzero` and is reported ([`Decoded::combined`]). The
//! writer gives every subpath the rule's operation.

use crate::path_geometry::{Anchor, FillRule, PathShape, Subpath};

/// One record's size.
const RECORD: usize = 26;
/// The largest magnitude an 8.24 fraction holds.
const FIXED_LIMIT: f64 = 128.0;
/// 2²⁴.
const ONE: f64 = 16_777_216.0;

/// A decoded path and what Doctored's model could not hold.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Decoded {
    pub shape: PathShape,
    /// The initial fill record says the path starts from all pixels.
    pub inverted: bool,
    /// The subpaths' operations are not all one of the two Doctored reads
    /// exactly (module doc).
    pub combined: bool,
    /// Each subpath's operation word, in order (`vector_mask_combine`).
    pub operations: Vec<i16>,
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

fn fixed(bytes: &[u8], at: usize) -> f64 {
    f64::from(i32::from_be_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
    ])) / ONE
}

/// The records of `bytes` (a whole number of 26-byte records; a trailing
/// part record, Photoshop's padding, is ignored) as a shape on a `canvas`.
pub(crate) fn decode(bytes: &[u8], canvas: (u32, u32)) -> Result<Decoded, String> {
    let (w, h) = (f64::from(canvas.0), f64::from(canvas.1));
    let mut subpaths: Vec<Subpath> = Vec::new();
    let mut pending = 0usize;
    let mut inverted = false;
    let mut operations: Vec<i16> = Vec::new();
    for record in bytes.chunks_exact(RECORD) {
        let selector = word(record, 0);
        match selector {
            0 | 3 => {
                if pending != 0 {
                    return Err("a path subpath has fewer knots than its length record".into());
                }
                pending = usize::from(word(record, 2));
                operations.push(word(record, 4) as i16);
                subpaths.push(Subpath {
                    anchors: Vec::with_capacity(pending.min(4096)),
                    closed: selector == 0,
                });
            }
            1 | 2 | 4 | 5 => {
                let subpath = subpaths
                    .last_mut()
                    .filter(|_| pending != 0)
                    .ok_or("a path knot has no subpath length record")?;
                if subpath.closed != matches!(selector, 1 | 2) {
                    return Err("a path knot does not match its subpath's kind".into());
                }
                let point = |at: usize| [fixed(record, at + 4) * w, fixed(record, at) * h];
                subpath.anchors.push(Anchor {
                    handle_in: point(2),
                    point: point(10),
                    handle_out: point(18),
                    smooth: matches!(selector, 1 | 4),
                });
                pending -= 1;
            }
            8 => inverted = word(record, 2) == 1,
            6 | 7 => {}
            _ => return Err("a path record has an unknown selector".into()),
        }
    }
    if pending != 0 {
        return Err("a path subpath has fewer knots than its length record".into());
    }
    let even = operations.iter().all(|&op| op == 0);
    let fill_rule = if !operations.is_empty() && even {
        FillRule::EvenOdd
    } else {
        FillRule::NonZero
    };
    Ok(Decoded {
        shape: PathShape {
            subpaths,
            fill_rule,
        },
        inverted,
        combined: !even && !operations.iter().all(|&op| op == 1),
        operations,
    })
}

/// The records of `shape` on a `canvas`, `inverted` the initial fill. `Err`
/// for a coordinate past the fixed-point range (module doc).
pub(crate) fn encode(
    shape: &PathShape,
    canvas: (u32, u32),
    inverted: bool,
) -> Result<Vec<u8>, String> {
    let (w, h) = (f64::from(canvas.0), f64::from(canvas.1));
    let mut out = Vec::with_capacity(RECORD * (2 + shape.anchor_count() + shape.subpaths.len()));
    let mut record = |selector: u16, body: &[u8]| {
        out.extend_from_slice(&selector.to_be_bytes());
        out.extend_from_slice(body);
        out.extend(std::iter::repeat_n(0, RECORD - 2 - body.len()));
    };
    record(6, &[]);
    record(8, &u16::from(inverted).to_be_bytes());
    let operation: i16 = match shape.fill_rule {
        FillRule::EvenOdd => 0,
        FillRule::NonZero => 1,
    };
    let to_fixed = |value: f64, side: f64| -> Result<[u8; 4], String> {
        let fraction = value / side;
        if !fraction.is_finite() || fraction.abs() >= FIXED_LIMIT {
            return Err("a path coordinate lies outside the range a PSD path can hold".into());
        }
        Ok(((fraction * ONE).round() as i32).to_be_bytes())
    };
    for subpath in &shape.subpaths {
        let count = u16::try_from(subpath.anchors.len())
            .map_err(|_| "a path subpath has too many anchors for a PSD path")?;
        let mut length = count.to_be_bytes().to_vec();
        length.extend_from_slice(&operation.to_be_bytes());
        length.extend_from_slice(&1u16.to_be_bytes());
        record(if subpath.closed { 0 } else { 3 }, &length);
        for anchor in &subpath.anchors {
            let mut knot = Vec::with_capacity(24);
            for p in [anchor.handle_in, anchor.point, anchor.handle_out] {
                knot.extend_from_slice(&to_fixed(p[1], h)?);
                knot.extend_from_slice(&to_fixed(p[0], w)?);
            }
            let selector = match (subpath.closed, anchor.smooth) {
                (true, true) => 1,
                (true, false) => 2,
                (false, true) => 4,
                (false, false) => 5,
            };
            record(selector, &knot);
        }
    }
    Ok(out)
}

/// Resource 2999's body as `(name bytes, tail)` (module doc). `None` for a
/// body that ends inside its Pascal string.
pub(crate) fn decode_clipping(body: &[u8]) -> Option<(&[u8], &[u8])> {
    let length = usize::from(*body.first()?);
    let name = body.get(1..1 + length)?;
    Some((name, &body[1 + length..]))
}

/// The tail an authored 2999 holds after the name when no flatness is set:
/// flatness 0 (16.16), then u16 1 (module doc).
const CLIPPING_TAIL_NONE: [u8; 6] = [0, 0, 0, 0, 0, 1];

/// The tail a document keeps for a file's `tail`: empty for the authored
/// no-flatness tail, the form a mark made in Doctored keeps (so a mark's
/// round trip through a file is itself), else the bytes verbatim.
pub(crate) fn kept_clipping_tail(tail: &[u8]) -> Vec<u8> {
    if tail == CLIPPING_TAIL_NONE {
        Vec::new()
    } else {
        tail.to_vec()
    }
}

/// [`decode_clipping`]'s inverse: `name` as a Pascal string, then `tail`, or
/// the no-flatness tail for an empty one (module doc). `None` for a name past
/// the Pascal string's 255 bytes.
pub(crate) fn encode_clipping(name: &[u8], tail: &[u8]) -> Option<Vec<u8>> {
    let length = u8::try_from(name.len()).ok()?;
    let tail = if tail.is_empty() {
        &CLIPPING_TAIL_NONE[..]
    } else {
        tail
    };
    let mut out = Vec::with_capacity(1 + name.len() + tail.len());
    out.push(length);
    out.extend_from_slice(name);
    out.extend_from_slice(tail);
    Some(out)
}

/// A vector mask's `vmsk` body: version 3, the flags word, the records.
/// `flags` bit 0 inverted, bit 1 not linked, bit 2 disabled.
pub(crate) fn decode_vmsk(body: &[u8], canvas: (u32, u32)) -> Result<(Decoded, u32), String> {
    if body.len() < 8 || body[..4] != 3u32.to_be_bytes() {
        return Err("the vector mask is not version 3".into());
    }
    let flags = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
    Ok((decode(&body[8..], canvas)?, flags))
}

/// [`decode_vmsk`]'s inverse, padded to a multiple of four as Photoshop's
/// own is.
pub(crate) fn encode_vmsk(
    shape: &PathShape,
    canvas: (u32, u32),
    flags: u32,
) -> Result<Vec<u8>, String> {
    let mut out = 3u32.to_be_bytes().to_vec();
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&encode(shape, canvas, flags & 1 != 0)?);
    out.resize(out.len().next_multiple_of(4), 0);
    Ok(out)
}
