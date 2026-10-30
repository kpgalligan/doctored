//! The TYPED content box: [`PrecisionDocument::layer_bounds`], the
//! twin of `DrDocument::layer_bounds` (`doc_group_query`) at 16-bit and
//! 32-bit float. Same `(x, y, width, height)` tuple, same rules: a raster
//! record's non-transparent pixels with its ENABLED mask applied; a group's
//! (isolated or Pass Through) union over its raster descendants, each with
//! its own mask (the group's mask is ignored, as the byte union ignores it);
//! `None` for an adjustment record, an out-of-range index and an entry with
//! nothing opaque. Not clipped to the canvas, blind to `visible`, and the
//! final tuple clamped exactly as the byte one. The index is the postorder
//! record index `PrecisionDocument::layer` reads. A CMYK document's raster
//! (#547 phase 4b) is the same rule over its straight alpha (`[4]`) × its
//! enabled mask ([`scan_cmyk`]), memoized on `CmykImage::identity_weak`.
//!
//! THE SCAN. An unmasked raster is `doc_export_layers::alpha_bounds16` /
//! `alpha_bounds32` — the trimmed export's own tile-band walk, alpha > 0,
//! uncontrolled here (`JobContext::NONE`; the memo has no job) — offset to
//! the canvas. A masked one is `doc_group_query::edge_bounds` (the
//! crate's one content-bounds scan, row by row, uncontrolled) over that alpha
//! window, each row read as the image's own tile runs (`row_segments`, either
//! storage) through `runs_extent`, a pixel counting when its alpha is > 0 AND
//! its effective coverage from `PrecisionMask::prepare` (feather and density
//! included) is > 0. A mask
//! whose feather is not prepared here — its plan's work is over the
//! interactive budget `precision_render::MAX_WORK_UNITS`
//! ([`prepared_within_budget`]: this scan has no job control, so nothing
//! could cancel a longer blur; a render feathers the same mask under its own
//! control, with no work refusal) — answers
//! the UNMASKED alpha box instead of `None`, so a caller (a group distort,
//! a snap fold) never reads "nothing opaque" off a layer that holds pixels.
//!
//! THE MEMO. The byte `CONTENT_BOX_MEMO`'s twin, for the same reason: the
//! snap fold, `get_document` and a group distort re-ask after every settled
//! edit, and an edit that touched one record leaves every other record's box
//! unchanged — while a feathered typed mask re-blurs on every miss. Keyed on
//! the image's identity token (`PrecisionImage::identity_key`: minted by
//! every constructor, so a changed image is a new key), the ENABLED mask
//! plane's `Arc` (a disabled mask keys as none, the way it reads) with its
//! dimensions, the record offset, and the mask properties that affect
//! coverage (offset, density, feather, outside coverage). Each is held as a
//! `Weak`, which keeps the allocation's control block alive, so a stored
//! address can never be reused while its entry lives and pointer equality is
//! exact. Bounded and least recently used, at the byte memo's capacity; an
//! entry pins no samples.
//!
//! `PrecisionDocument::layer_bounds`, the 16/32-bit twin of the byte `DrDocument::layer_bounds`
//! (`doc_group_query.rs`) with the same tuple and rules (a raster's alpha × its enabled mask; a
//! group's union over its raster descendants, its own mask ignored; `None` for an adjustment, a bad
//! index or nothing opaque). Its memo is `CONTENT_BOX_MEMO`'s twin, keyed on the image's
//! `identity_key` and the enabled mask plane's `Arc`, both held as `Weak`s (no samples pinned),
//! plus the record offset and the coverage-affecting mask properties.

use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::cmyk_image::CmykImage;
use crate::doc_export_layers::{alpha_bounds16, alpha_bounds32};
use crate::doc_group_query::{clamp_i32, edge_bounds, runs_extent, union};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{PrecisionImage, PrecisionRun, PrecisionStorage};
use crate::precision_mask::{PrecisionMask, PreparedMask};
use crate::precision_render::MAX_WORK_UNITS;

impl PrecisionDocument {
    /// `DrDocument::layer_bounds`' typed twin: the canvas box of record
    /// `idx`'s visible CONTENT, with the same tuple, clamps and `None` rules
    /// (module doc).
    pub fn layer_bounds(&self, idx: usize) -> Option<(i32, i32, u32, u32)> {
        let record = self.layer(idx)?;
        let mut box_ = None;
        match &record.content {
            Content::Raster { .. } | Content::CmykRaster { .. } => {
                union(&mut box_, content_box(record))
            }
            Content::Adjustment(_) | Content::CmykAdjustment(_) => return None,
            Content::Group(children) | Content::PassThrough(children) => {
                descendants(children, &mut box_);
            }
        }
        let (x0, y0, x1, y1) = box_?;
        Some((
            clamp_i32(x0),
            clamp_i32(y0),
            (x1 - x0).clamp(0, i64::from(u32::MAX)) as u32,
            (y1 - y0).clamp(0, i64::from(u32::MAX)) as u32,
        ))
    }
}

/// The union over every raster record below a group, nested groups walked.
fn descendants(children: &[PrecisionDocumentLayer], box_: &mut Option<(i64, i64, i64, i64)>) {
    for child in children {
        match &child.content {
            Content::Raster { .. } | Content::CmykRaster { .. } => union(box_, content_box(child)),
            Content::Adjustment(_) | Content::CmykAdjustment(_) => {}
            Content::Group(inner) | Content::PassThrough(inner) => descendants(inner, box_),
        }
    }
}

/// `mask` prepared with no job control, or `None` when its feather's planned
/// work is over [`MAX_WORK_UNITS`] or it cannot be prepared at all: the
/// content box and the hit test (`precision_layer_hit`) then read the record
/// as unmasked (module doc).
pub(crate) fn prepared_within_budget(mask: &PrecisionMask) -> Option<PreparedMask<'_>> {
    let (_, work) = mask.plan().ok()?;
    if work > MAX_WORK_UNITS {
        return None;
    }
    mask.prepare(JobContext::NONE).ok()
}

/// The box `(x0, y0, x1, y1)` (exclusive, canvas coordinates) of a raster
/// record's non-transparent pixels, its enabled mask applied (module doc);
/// `None` when none are. [`content_box`] fronts this with the memo.
fn scan(
    image: &PrecisionImage,
    offset: (i32, i32),
    mask: Option<&PrecisionMask>,
) -> Option<(i64, i64, i64, i64)> {
    let (ax, ay, aw, ah) = match image.storage() {
        PrecisionStorage::Rgba16Unorm => alpha_bounds16(image, JobContext::NONE).ok()?,
        PrecisionStorage::Rgba32Float => alpha_bounds32(image, JobContext::NONE).ok()?,
    }?;
    let (ox, oy) = (i64::from(offset.0), i64::from(offset.1));
    let alpha_box = (ax, ay, ax + aw, ay + ah);
    let (x0, y0, x1, y1) = match mask.and_then(prepared_within_budget) {
        // No enabled mask, or one whose feather is not prepared here: the
        // unmasked alpha box.
        None => alpha_box,
        Some(prepared) => {
            let rows = |y: u32, x0: u32, x1: u32| {
                let by = ay + y;
                let kept = |bx: u32| prepared.at(ox + i64::from(bx), oy + i64::from(by)) > 0.0;
                let runs = image.row_segments(by, ax + x0, ax + x1);
                let (first, last) = match image.storage() {
                    PrecisionStorage::Rgba16Unorm => runs_extent(
                        runs.filter_map(|(x, run)| match run {
                            PrecisionRun::Rgba16(run) => Some((x, run)),
                            PrecisionRun::Rgba32(_) => None,
                        }),
                        |bx, p| p[3] > 0 && kept(bx),
                    ),
                    PrecisionStorage::Rgba32Float => runs_extent(
                        runs.filter_map(|(x, run)| match run {
                            PrecisionRun::Rgba32(run) => Some((x, run)),
                            PrecisionRun::Rgba16(_) => None,
                        }),
                        |bx, p| p[3] > 0.0 && kept(bx),
                    ),
                }?;
                Some((first - ax, last - ax))
            };
            let (x0, y0, x1, y1) = edge_bounds(aw, ah, JobContext::NONE, rows).ok()??;
            (ax + x0, ay + y0, ax + x1, ay + y1)
        }
    };
    Some((
        ox + i64::from(x0),
        oy + i64::from(y0),
        ox + i64::from(x1),
        oy + i64::from(y1),
    ))
}

// ------------------------------------------------------------- the memo --

/// Entries kept: the byte memo's `CONTENT_BOX_MEMO_CAPACITY`.
const MEMO_CAPACITY: usize = 512;

/// The mask properties that change coverage: offset, density bits, feather
/// bits, outside coverage.
type MaskKey = ((i32, i32), u32, u32, u8);

/// The ENABLED mask plane's allocation and its dimensions.
type PlaneKey = (Weak<Vec<u16>>, (u32, u32));

/// One memo slot: the image identity, the ENABLED mask plane (`None` when
/// the record has none or it is disabled) with its dimensions, the record
/// offset, the mask properties, and the box.
struct Entry {
    image: Weak<()>,
    mask: Option<PlaneKey>,
    offset: (i32, i32),
    properties: MaskKey,
    box_: Option<(i64, i64, i64, i64)>,
}

static MEMO: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();

/// Clears the typed memo (`resources::evict_caches`, beside the byte one).
/// It owns no samples.
pub(crate) fn evict_precision_content_box_cache() {
    if let Some(memo) = MEMO.get() {
        memo.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

/// [`scan`] (or [`scan_cmyk`]) behind the memo. Transparent: the same key
/// always gives the same box, and a poisoned lock simply scans again. `None`
/// for a non-raster.
fn content_box(record: &PrecisionDocumentLayer) -> Option<(i64, i64, i64, i64)> {
    let mask = record.mask.as_ref().filter(|m| m.enabled());
    match &record.content {
        Content::Raster { image, offset } => memoized(image.identity_key(), offset, mask, || {
            scan(image, *offset, mask)
        }),
        Content::CmykRaster { image, offset } => {
            memoized(image.identity_weak(), offset, mask, || {
                scan_cmyk(image, *offset, mask)
            })
        }
        _ => None,
    }
}

/// The memo lookup and insert around one record's `scan`.
fn memoized(
    key: Weak<()>,
    offset: &(i32, i32),
    mask: Option<&PrecisionMask>,
    scan: impl FnOnce() -> Option<(i64, i64, i64, i64)>,
) -> Option<(i64, i64, i64, i64)> {
    let properties = mask.map_or(((0, 0), 0, 0, 0), |m| {
        let p = m.properties();
        (
            p.offset,
            p.density.to_bits(),
            p.feather.to_bits(),
            p.outside_coverage,
        )
    });
    let matches = |entry: &Entry| {
        entry.image.ptr_eq(&key)
            && match (&entry.mask, mask) {
                (None, None) => true,
                (Some((weak, dims)), Some(m)) => {
                    std::ptr::eq(weak.as_ptr(), Arc::as_ptr(&m.plane.pixels))
                        && *dims == m.plane.dimensions()
                }
                _ => false,
            }
            && entry.offset == *offset
            && entry.properties == properties
    };
    let memo = MEMO.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut entries) = memo.lock() {
        if let Some(i) = entries.iter().position(&matches) {
            // Moved to the back, so eviction is least recently USED.
            let hit = entries.remove(i);
            let box_ = hit.box_;
            entries.push(hit);
            return box_;
        }
    }
    // Scanned OUTSIDE the lock, as the byte memo does.
    let box_ = scan();
    if let Ok(mut entries) = memo.lock() {
        entries.retain(|entry| entry.image.strong_count() > 0);
        if !entries.iter().any(&matches) {
            while entries.len() >= MEMO_CAPACITY {
                entries.remove(0);
            }
            entries.push(Entry {
                image: key.clone(),
                mask: mask.map(|m| (Arc::downgrade(&m.plane.pixels), m.plane.dimensions())),
                offset: *offset,
                properties,
                box_,
            });
        }
    }
    box_
}

/// A CMYK record's box (#547 phase 4b): [`scan`]'s rule over straight
/// CMYKA16 — a pixel counts when its alpha (`[4]`) is > 0 and its enabled
/// mask's prepared coverage is > 0 — through the same `edge_bounds` /
/// `runs_extent` scan over the image's own tile runs. (The RGB unmasked
/// alpha box's tile-band walk, `alpha_bounds16`, is RGBA-only; this scan
/// reads the same pixels.) A mask not prepared within budget reads as none.
fn scan_cmyk(
    image: &CmykImage,
    offset: (i32, i32),
    mask: Option<&PrecisionMask>,
) -> Option<(i64, i64, i64, i64)> {
    let (w, h) = image.dimensions();
    let (ox, oy) = (i64::from(offset.0), i64::from(offset.1));
    let prepared = mask.and_then(prepared_within_budget);
    let rows = |y: u32, x0: u32, x1: u32| {
        let kept = |x: u32| {
            prepared
                .as_ref()
                .is_none_or(|p| p.at(ox + i64::from(x), oy + i64::from(y)) > 0.0)
        };
        runs_extent(image.row_segments(y, x0, x1), |x, p| p[4] > 0 && kept(x))
    };
    let (x0, y0, x1, y1) = edge_bounds(w, h, JobContext::NONE, rows).ok()??;
    Some((
        ox + i64::from(x0),
        oy + i64::from(y0),
        ox + i64::from(x1),
        oy + i64::from(y1),
    ))
}
