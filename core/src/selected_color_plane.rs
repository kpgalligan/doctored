//! MOVE AND FREE TRANSFORM OF ONE COLOUR CHANNEL'S SELECTED PIXELS:
//! the Move tool, its nudge, ⌘T / ⌥⌘T, ⇧⌘T and `move_selected_pixels` /
//! `transform_selected_pixels` with `"target":"red"|"green"|"blue"|"alpha"`,
//! at 8 bits (u8), 16 bits (u16) and 32-bit float (f32). This module is an
//! adapter: it extracts the component over the layer rect R, runs
//! `selected_plane`'s `Source` and THE walk (`selected_pixels_walk`) over it,
//! and writes the component back ([`walk_component`], which a CMYK
//! document's ink plate runs too: its twin `cmyk_selected_plate`). The f32 arm rides the extended path
//! (`StoredSample::EXTENDED`, `landing_extended`): nothing quantizes and
//! above-white or negative values are kept.
//!
//! Decisions (measured against Photoshop 26.10.0, take1 and take2,
//! `scripts/photoshop-probe/channel_move.py` and `channel_plane.py`; the
//! vacated value from `plane_gray.py`, `plane-gray-431-633/take1`):
//!
//! * ONE COMPONENT MOVES. The landing writes only component k; the other
//!   components and the alpha stay bit-identical (take1: `others` untouched,
//!   `clear` kept), and a colour plane never grows the layer: every write is
//!   clipped to R.
//! * THE VACATED AREA of Red, Green or Blue is the background swatch's
//!   30/59/11 luma rounded to an 8-bit code, over 255, at every depth, the
//!   32-bit value unconverted (`plane_gray.py`: `hole8`, `hole16` and
//!   `hole32` `3059r`, `form32` `raw`; take2's `bg.601f` gave the same codes
//!   on its own three pairs; `plane_swatch::color_plane_hole`). It is
//!   `Hole::Fill`, `lerp(v, hole, c)`.
//!   The request carries the swatch's 8-bit codes (`"swatch"`, black when
//!   absent). Alpha's vacated value is 0 at every depth: Doctored's
//!   decision, a transparent hole, because Photoshop has no
//!   layer-transparency channel to measure. A `duplicate` leaves no hole.
//! * THE LIFT (take2's `lift` case, LIFT row `refused-empty`, the L-NOTHING
//!   build). A colour-channel lift from only transparent pixels refuses with
//!   "The selection holds no pixels in this colour channel"
//!   ([`COLOR_EMPTY_AREA`]) at 8, 16 and 32 bits (take2's LIFT row records
//!   the same refusal): for Red, Green and Blue the coverage the
//!   walk receives is scaled by the layer's stored
//!   alpha (0 outside R) into a canvas-sized u16 copy: a transparent source
//!   floats nothing, and an all-transparent one is the walk's empty float.
//!   Partial alpha (0 < α < 1) is unmeasured and follows the same scaling.
//!   An Alpha lift reads the stored alpha with the coverage unscaled, and 0
//!   outside R.
//! * LOCKS. Red, Green and Blue are `EditKind::Pixels` (Lock Pixels and Lock
//!   All refuse, Lock Transparency allows); Alpha is `EditKind::Merge` (Lock
//!   Transparency refuses too). A group or adjustment layer refuses with the
//!   layer path's `target_refusal`, a retained source with its
//!   `retained_refusal`; the typed publish clears a description
//!   (`replacing_content`).
//!
//! # Admission
//!
//! Plan = `plane_scratch(edit, canvas, R, S)` + |R|·(S + W), plus 2·|canvas|
//! for the alpha-scaled coverage copy of a Red, Green or Blue target: S = 1, 2
//! or 4 (the plane sample), W = 8 at u8, 2·`sample_bytes(Rgba16Unorm)` at u16
//! and `FLOAT_SAMPLE_BYTES` at f32 (the RGBA read and the rebuilt tiles).
//! The walk runs with the layer's RGBA dropped (it is read again for the
//! write-back), so its peak holds only the plane, the coverage copy and the
//! walk's own buffers.
//!
//! Measured in Release on 6000×4000 opaque documents (`float_adjust_apply_24mp`'s
//! RZCB ramps at f32, the same ramps clamped to 0…1 as RGBA16 and RGBA8), a canvas
//! coverage of rect (300, 200, 5400, 3600) at 65535 and `"target":"red"` with
//! `"swatch":[220,30,170]`, through `dr_doc_selected_pixels_edit` (8) and
//! `dr_precision_doc_edit` (16, 32) at limit 8,589,934,592 with the result held until
//! E is read; one case per process, `MallocLargeCache=0`, three runs. E =
//! `ri_interval_max_phys_footprint` − `ri_phys_footprint` read right after
//! `proc_reset_footprint_interval`; A = the plan's `scratch_bytes`. M = `move_selected`
//! dx 37 dy 23 with `bounds` = that rect; T = `transform_selected` 7° about the canvas
//! centre, bilinear, the same `bounds`. The baseline twin (same setup, no call) read
//! E 0 for every case. Apple M2 Max, 96 GiB, 2026-10-10.
//!
//! | case | A | E ×3 | A / max E |
//! |---|---|---|---|
//! | M8 | 622,932,555 | 172,949,624 · 172,933,240 · 172,933,240 | 3.602 |
//! | M16 | 882,630,806 | 292,995,264 · 292,978,880 · 292,978,880 | 3.012 |
//! | M32 | 1,786,027,308 | 736,510,480 · 736,510,480 · 736,526,864 | 2.425 |
//! | T8 | 997,770,304 | 779,092,520 · 779,092,520 · 779,092,520 | 1.281 |
//! | T16 | 1,260,978,304 | 822,575,704 · 822,575,704 · 822,575,704 | 1.533 |
//! | T32 | 2,171,394,304 | 909,476,488 · 909,476,488 · 909,476,488 | 2.388 |
//!
//! Each clears the ⌈1.25 × max E⌉ gate (216,187,030; 366,244,080; 920,658,580;
//! 973,865,650; 1,028,219,630; 1,136,845,610), which
//! `tests/selected_color_planes_24mp.rs` pins.

use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::plane_swatch::color_plane_hole;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, reserved_vec, PrecisionStorage};
use crate::precision_lock::named;
use crate::precision_selected_pixels::{sample_bytes, FLOAT_SAMPLE_BYTES};
use crate::selected_pixels::{
    area, CanvasRect, Hole, Landing, SelectedPixelsEdit, SelectedPixelsPlan,
};
use crate::selected_pixels_walk::StoredSample;
use crate::selected_plane::{plane_scratch, Source};

/// The refusal of a colour-channel lift with nothing in it (Doctored's
/// sentence; take2's LIFT row records the same refusal), the sentence
/// both a move and a transform of a colour plane give.
pub(crate) const COLOR_EMPTY_AREA: &str = "The selection holds no pixels in this colour channel.";
const COVERAGE_LENGTH: &str = "selected pixels coverage must be canvas-sized u16";
const NO_PLANE: &str = "could not build the transformed colour channel";

fn read_failed() -> String {
    "could not allocate the moved selected pixels".into()
}

/// A 32-bit float colour-plane sample: an opaque gray, as the u8 and
/// u16 plane samples are, with nothing quantized and extended values kept.
impl StoredSample for f32 {
    const EXTENDED: bool = true;
    fn unit(self) -> [f32; 4] {
        [self, self, self, 1.0]
    }
    fn store(value: [f32; 4]) -> Self {
        value[0]
    }
    fn alpha(self) -> bool {
        true
    }
}

/// The component k of a target, or the refusal for a non-colour one.
fn component(edit: &SelectedPixelsEdit) -> Result<usize, String> {
    edit.target().component().ok_or_else(|| NO_PLANE.into())
}

/// The edit kind a colour plane's lock check asks: Pixels for R/G/B, Merge
/// for Alpha (it writes the layer's transparency).
fn edit_kind(k: usize) -> EditKind {
    if k == 3 {
        EditKind::Merge
    } else {
        EditKind::Pixels
    }
}

/// The colour plane's hole: none for a duplicate, the swatch's built value
/// for R/G/B, and Alpha's 0 (`SelectedPixelsEdit::hole`).
fn hole(edit: &SelectedPixelsEdit, k: usize) -> Hole {
    if edit.duplicate() || k == 3 {
        return edit.hole();
    }
    Hole::Fill(color_plane_hole(edit.swatch().unwrap_or([0, 0, 0])))
}

/// The plan's scratch: the plane walk over V, the extracted and written
/// component over R at `s` bytes, the RGBA read and rebuild at `w`, and the
/// alpha-scaled coverage copy of an R/G/B target.
fn scratch(
    edit: &SelectedPixelsEdit,
    canvas: (u32, u32),
    rect: CanvasRect,
    s: u64,
    w: u64,
) -> Result<u64, String> {
    let k = component(edit)?;
    let copy = if k < 3 {
        2u64.saturating_mul(u64::from(canvas.0) * u64::from(canvas.1))
    } else {
        0
    };
    Ok(plane_scratch(edit, canvas, rect, s)?
        .saturating_add(area(rect).saturating_mul(s.saturating_add(w)))
        .saturating_add(copy))
}

/// The coverage an R/G/B walk receives: `coverage` × the stored alpha
/// `alpha(i)` (0…1, `i` the R-local index), 0 outside R.
fn alpha_scaled(
    coverage: &[u16],
    canvas: (u32, u32),
    rect: CanvasRect,
    alpha: impl Fn(usize) -> f32,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let mut out = reserved_vec(coverage.len())?;
    out.resize(coverage.len(), 0);
    for y in 0..canvas.1 {
        job.check()?;
        let ly = i64::from(y) - rect.1;
        if ly < 0 || ly >= i64::from(rect.3) {
            continue;
        }
        let row = y as usize * canvas.0 as usize;
        for x in 0..canvas.0 {
            let lx = i64::from(x) - rect.0;
            if lx < 0 || lx >= i64::from(rect.2) {
                continue;
            }
            let c = coverage[row + x as usize];
            if c == 0 {
                continue;
            }
            let a = alpha(ly as usize * rect.2 as usize + lx as usize).clamp(0.0, 1.0);
            out[row + x as usize] = (f32::from(c) * a).round() as u16;
        }
    }
    Ok(out)
}

/// Which component one walk moves: its index `k` in the stored pixel, the
/// hole it leaves, and whether the coverage is scaled by the stored alpha
/// (R/G/B here; an ink plate in `cmyk_selected_plate`).
pub(crate) struct ComponentPlane {
    pub(crate) k: usize,
    pub(crate) hole: Hole,
    pub(crate) scaled: bool,
}

impl ComponentPlane {
    /// An RGB colour-plane target's component (module doc).
    fn of(edit: &SelectedPixelsEdit) -> Result<Self, String> {
        let k = component(edit)?;
        Ok(Self {
            k,
            hole: hole(edit, k),
            scaled: k < 3,
        })
    }
}

/// Reads the layer's pixels over R (`read`), extracts component k and, when
/// `plane.scaled`, the alpha-scaled coverage, then DROPS the pixels before
/// THE walk runs, so the walk's peak never holds the layer's samples
/// (`# Admission`); after the walk it reads R again and writes component k
/// back. `None` when no sample of the component changed. `alpha` normalizes
/// a stored alpha for the coverage scale. THE walk for one stored component:
/// the RGB planes here and a CMYK document's ink plate
/// (`cmyk_selected_plate`, its twin over `[u16; 5]`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn walk_component<P: Copy, S: StoredSample + PartialEq>(
    edit: &SelectedPixelsEdit,
    canvas: (u32, u32),
    rect: CanvasRect,
    coverage: &[u16],
    landing: &Landing,
    plane: ComponentPlane,
    read: impl Fn() -> Result<Vec<P>, String>,
    get: impl Fn(&P, usize) -> S,
    set: impl Fn(&mut P, usize, S),
    alpha: impl Fn(&P) -> f32,
    job: JobContext<'_>,
) -> Result<Option<Vec<P>>, String> {
    let ComponentPlane {
        k,
        hole,
        scaled: scale,
    } = plane;
    let (plane, scaled) = {
        let rgba = read()?;
        let mut plane = reserved_vec(rgba.len())?;
        plane.extend(rgba.iter().map(|p| get(p, k)));
        let scaled = if scale {
            Some(alpha_scaled(
                coverage,
                canvas,
                rect,
                |i| alpha(&rgba[i]),
                job,
            )?)
        } else {
            None
        };
        (plane, scaled)
    };
    let source = Source {
        rect,
        samples: &plane,
        outside: S::default(),
        grow: false,
    };
    let walked = scaled.as_deref().unwrap_or(coverage);
    let Some((written, samples)) = source.transform(edit, canvas, walked, landing, hole, job)?
    else {
        return Ok(None);
    };
    if written != rect || samples.len() != plane.len() {
        return Err(NO_PLANE.into());
    }
    drop(plane);
    drop(scaled);
    let mut rgba = read()?;
    for (pixel, value) in rgba.iter_mut().zip(samples) {
        set(pixel, k, value);
    }
    Ok(Some(rgba))
}

impl DrDocument {
    /// The byte plan of a colour-plane target: sample-free; validates the
    /// layer target and a transform's geometry. No lock check.
    pub(crate) fn selected_color_plane_plan(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<SelectedPixelsPlan, String> {
        let entry = self.selected_pixels_target(edit)?;
        let (w, h) = entry.pixels.dimensions();
        let rect = (i64::from(entry.offset.0), i64::from(entry.offset.1), w, h);
        let canvas = (self.width, self.height);
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        Ok(SelectedPixelsPlan {
            retained_bytes: inventory.unique_bytes(),
            scratch_bytes: scratch(edit, canvas, rect, 1, 8)?,
            input_samples: u64::from(canvas.0) * u64::from(canvas.1),
        })
    }

    /// Move or transform component k of the layer's selected pixels.
    /// Refuses, in order: the coverage length, the target, a retained source,
    /// a lock (Pixels for R/G/B, Merge for Alpha); then the identity is
    /// `Ok(None)`; then the plan's admission and reservation, and an empty
    /// float ([`COLOR_EMPTY_AREA`]). `Ok(None)` when no stored sample changed.
    pub(crate) fn selected_color_plane_edit(
        &self,
        edit: &SelectedPixelsEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let canvas = (self.width, self.height);
        if coverage.len() as u64 != u64::from(canvas.0) * u64::from(canvas.1) {
            return Err(COVERAGE_LENGTH.into());
        }
        let index = edit.layer();
        let k = component(edit)?;
        let entry = self.selected_pixels_target(edit)?;
        if self.has_retained_source(index) {
            return Err(edit.retained_refusal());
        }
        let blocking = self.lock_block(index, edit_kind(k));
        if blocking != 0 {
            let (bit, which) = named(blocking);
            return Err(match self.lock_owner(index, bit) {
                Some(owner) if owner != index => {
                    format!("layer {index} is inside group {owner}, which is locked ({which})")
                }
                _ => format!("layer {index} is locked ({which})"),
            });
        }
        let Some(landing) = edit.landing(canvas)? else {
            return Ok(None);
        };
        let plan = self.selected_color_plane_plan(edit)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        job.check()?;
        let (w, h) = entry.pixels.dimensions();
        let rect = (i64::from(entry.offset.0), i64::from(entry.offset.1), w, h);
        let pixels = &entry.pixels;
        let read = || pixels.read_rect(0, 0, w, h).ok_or_else(read_failed);
        let Some(rgba) = walk_component::<[u8; 4], u8>(
            edit,
            canvas,
            rect,
            coverage,
            &landing,
            ComponentPlane::of(edit)?,
            read,
            |p, k| p[k],
            |p, k, v| p[k] = v,
            |p| f32::from(p[3]) / 255.0,
            job,
        )?
        else {
            return Ok(None);
        };
        let Some(raster) = entry.pixels.with_rect(0, 0, w, h, &rgba)? else {
            return Ok(None);
        };
        let mut next = self.clone();
        next.layers[index].pixels = raster;
        job.check()?;
        Ok(Some(next))
    }
}

impl PrecisionDocument {
    /// The typed twin of `DrDocument::selected_color_plane_plan`: S = 2 and
    /// W = 2·`sample_bytes` at u16, S = 4 and W = `FLOAT_SAMPLE_BYTES` at f32.
    pub(crate) fn selected_color_plane_plan(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<SelectedPixelsPlan, String> {
        let (image, rect) = self.selected_pixels_target(edit)?;
        let canvas = self.dimensions();
        let (s, w) = match image.storage() {
            PrecisionStorage::Rgba16Unorm => (2, 2 * sample_bytes(PrecisionStorage::Rgba16Unorm)),
            PrecisionStorage::Rgba32Float => (4, FLOAT_SAMPLE_BYTES),
        };
        Ok(SelectedPixelsPlan {
            retained_bytes: self.retained_bytes(),
            scratch_bytes: scratch(edit, canvas, rect, s, w)?,
            input_samples: u64::from(canvas.0) * u64::from(canvas.1),
        })
    }

    /// The typed twin of `DrDocument::selected_color_plane_edit`, same
    /// refusals in the same order; the publication clears a description.
    pub(crate) fn selected_color_plane_with_context(
        &self,
        edit: &SelectedPixelsEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let canvas = self.dimensions();
        if coverage.len() as u64 != u64::from(canvas.0) * u64::from(canvas.1) {
            return Err(COVERAGE_LENGTH.into());
        }
        let index = edit.layer();
        let k = component(edit)?;
        let (image, rect) = self.selected_pixels_target(edit)?;
        if self.has_retained_source(index) {
            return Err(edit.retained_refusal());
        }
        self.lock_refusal(index, edit_kind(k))?;
        let Some(landing) = edit.landing(canvas)? else {
            return Ok(None);
        };
        let plan = self.selected_color_plane_plan(edit)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        job.check()?;
        let (w, h) = (rect.2, rect.3);
        let published = match image.storage() {
            PrecisionStorage::Rgba16Unorm => {
                let read = || image.read_rect_rgba16(0, 0, w, h).ok_or_else(read_failed);
                match walk_component::<[u16; 4], u16>(
                    edit,
                    canvas,
                    rect,
                    coverage,
                    &landing,
                    ComponentPlane::of(edit)?,
                    read,
                    |p, k| p[k],
                    |p, k, v| p[k] = v,
                    |p| f32::from(p[3]) / 65535.0,
                    job,
                )? {
                    Some(rgba) => image.with_rect_rgba16(0, 0, w, h, &rgba)?,
                    None => None,
                }
            }
            PrecisionStorage::Rgba32Float => {
                let read = || image.read_rect_rgba32(0, 0, w, h).ok_or_else(read_failed);
                match walk_component::<[f32; 4], f32>(
                    edit,
                    canvas,
                    rect,
                    coverage,
                    &landing,
                    ComponentPlane::of(edit)?,
                    read,
                    |p, k| p[k],
                    // The stored alpha stays in 0…1 (`extended_pixel`).
                    |p, k, v| p[k] = if k == 3 { v.clamp(0.0, 1.0) } else { v },
                    |p| p[3],
                    job,
                )? {
                    Some(rgba) => image.with_rect_rgba32(0, 0, w, h, &rgba)?,
                    None => None,
                }
            }
        };
        let Some(image) = published else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let offset = (rect.0 as i32, rect.1 as i32);
        let result = self.replacing_content(index, Content::Raster { image, offset })?;
        job.report(1.0, 5)?;
        Ok(result)
    }
}
