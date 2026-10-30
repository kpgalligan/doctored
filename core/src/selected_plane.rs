//! FREE TRANSFORM OF A SELECTED MASK OR CHANNEL AREA: ⌘T with a
//! selection while a layer mask or an alpha channel is the edit target, and
//! the MOVE of the same area (the Move tool, its nudge and
//! `move_selected_pixels`: `MoveSelected` with the same `target` and
//! `background`, whose landing is `Landing::Offset`, so it adds no landing
//! term to the plan and equals the transform's integer translation). The
//! plane's selected coverage floats, resamples through the transform's box and lands
//! back, at 8 bits (u8 planes) and at 16 and 32-bit float (u16 planes). This
//! module supplies only the plane's `read`, the walked rect `V`, the hole and
//! the writer; the float, the resample, the hole and the landing are
//! `selected_pixels_walk`'s ONE walk, read from the ORIGINAL and quantized
//! ONCE.
//!
//! Decisions:
//!
//! * THE VACATED AREA of a plane without transparency (a Background layer,
//!   a channel, a layer mask): a moved selection leaves the BACKGROUND
//!   COLOUR. The hole is `lerp(v, bg, c)` (`Hole::Fill`),
//!   `bg` the background swatch as coverage gray (the host's
//!   `PlaneAlgebra.coverageColor`, the Fill/Gradient plane rule), the
//!   swatch's alpha ignored; the request's `background` is that gray as u16
//!   (0 when absent). A `duplicate` leaves no hole.
//! * FLOAT AND LANDING. A plane sample is an opaque gray `[v, v, v, 1]`
//!   (`StoredSample` below), so the float is premultiplied `(v·c, c)` through
//!   `SelectedTransform` (the same map and `exact_form` rules),
//!   unpremultiplied and clamped, and lands as `lerp(holed, v_f, a_f)` —
//!   source-over onto an opaque pixel — quantized once. An integer translate
//!   of a hard selection is byte/code-exact.
//! * EDGES. The float's alpha IS the selection coverage: feathered and
//!   anti-aliased edges carry through, a hard edge gets the filter's fringe,
//!   Nearest stays hard. Every selected plane pixel floats (there is no
//!   transparency), so the host's box is the selection's bounds.
//! * EXTENT. The walk runs over `V = R ∪ canvas`, R the mask's stored rect
//!   (its `mask_properties.offset` + dimensions, as `doc_scope` maps it) or
//!   the canvas for a channel; samples outside R read as the mask's
//!   `outside_coverage`. Off-canvas landing is discarded, so the walk always
//!   answers `MovedSamples::Rect`. A channel stays canvas-sized. A raster or
//!   adjustment mask's storage grows to `R ∪ ((F ∪ landing) ∩ canvas)`, the
//!   new area at `outside_coverage`, and its offset moves only on growth. A
//!   GROUP mask keeps its storage (its offset positions it, `doc_mask_set`)
//!   and the write is clipped to R. Link, density, feather, enabled,
//!   outside coverage, layer pixels, comps and the warp binding are untouched.
//! * LOCKS. A mask edit is `EditKind::Mask` (only Lock All refuses, Lock
//!   Pixels allows); a channel has no lock. A retained source, a text/shape
//!   description, a group, an adjustment and float storage never refuse: the
//!   layer's pixels are untouched.
//! * The whole-plane mask transform (`mask_transform`) is unchanged; this is
//!   the selected-area twin.
//!
//! `TransformSelected` / `MoveSelected` with `"target":"mask"|{"channel":N}` and `"background":u16`
//! (`SelectedTarget`; `background` with a layer target refuses at parse), dispatched at the top of
//! both depths' plan/edit. It supplies only the plane's `read` (outside the stored rect R →
//! `outside_coverage`), `V = R ∪ canvas`, `Hole::Fill(bg)` (`lerp(v, bg, c)`, alpha kept —
//! `moved_pixel`'s `fill`) and the writer to THE walk (`StoredSample` for `u8`/`u16` as opaque
//! gray); a raster/adjustment mask grows to `R ∪ window` at its outside coverage, a group mask is
//! clipped to R, a channel stays canvas-sized; Lock All refuses a mask (`EditKind::Mask`), a
//! channel has no lock, float storage, retained sources and descriptions never refuse. Plan:
//! `plan_scratch` + `landing_scratch` (or `bounded_scratch`) over V plus `S·|V|` (S = 1 at 8 bits,
//! 2 at 16/32); a move's landing term is 0. Families `selected_pixels_transform` and
//! `selected_pixels_move` target `mask`/`channel` at 16 and in the float table. Tests:
//! `tests/transform_selected_planes.rs`. `Source`, its `transform` (which takes the hole) and
//! `plane_scratch` are `pub(crate)` for the colour planes (`selected_color_plane`), which run
//! the same walk over one component of a layer.

use std::sync::Arc;

use image::GrayImage;

use crate::doc::{DrDocument, LayerKind};
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, reserved_vec};
use crate::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use crate::selected_pixels::{
    area, intersect, move_samples, plan_scratch, union, CanvasRect, Hole, Landing, MovedSamples,
    SelectedPixelsEdit, SelectedPixelsPlan, SelectedTarget,
};
use crate::selected_pixels_float::bounded_scratch;
use crate::selected_pixels_walk::StoredSample;

const COVERAGE_LENGTH: &str = "selected pixels coverage must be canvas-sized u16";
const NO_PLANE: &str = "could not build the transformed plane";

/// An 8-bit plane sample: an opaque gray. `store` takes the gray from red.
impl StoredSample for u8 {
    fn unit(self) -> [f32; 4] {
        let v = f32::from(self) / 255.0;
        [v, v, v, 1.0]
    }
    fn store(value: [f32; 4]) -> Self {
        (value[0].clamp(0.0, 1.0) * 255.0).round() as u8
    }
    fn alpha(self) -> bool {
        true
    }
}

/// A 16-bit (and 32-bit float document) plane sample, as `u8`'s.
impl StoredSample for u16 {
    fn unit(self) -> [f32; 4] {
        let v = f32::from(self) / 65535.0;
        [v, v, v, 1.0]
    }
    fn store(value: [f32; 4]) -> Self {
        (value[0].clamp(0.0, 1.0) * 65535.0).round() as u16
    }
    fn alpha(self) -> bool {
        true
    }
}

/// The targeted plane: its stored canvas rect R, its samples, the coverage
/// outside R, and whether its storage may grow (not a channel, not a group
/// mask). `selected_color_plane` builds one over a layer's colour
/// component.
pub(crate) struct Source<'a, S> {
    pub(crate) rect: CanvasRect,
    pub(crate) samples: &'a [S],
    pub(crate) outside: S,
    pub(crate) grow: bool,
}

fn contains(rect: CanvasRect, (x, y): (i64, i64)) -> bool {
    x >= rect.0 && y >= rect.1 && x < rect.0 + i64::from(rect.2) && y < rect.1 + i64::from(rect.3)
}

impl<S: StoredSample + PartialEq> Source<'_, S> {
    fn at(&self, x: i64, y: i64) -> S {
        if !contains(self.rect, (x, y)) {
            return self.outside;
        }
        let r = self.rect;
        self.samples[(y - r.1) as usize * r.2 as usize + (x - r.0) as usize]
    }

    /// The walk's `read`: a rect in `v`'s own coordinates.
    fn read(&self, v: CanvasRect, x: u32, y: u32, w: u32, h: u32) -> Option<Vec<S>> {
        let mut out = reserved_vec(w as usize * h as usize).ok()?;
        for row in 0..h {
            let cy = v.1 + i64::from(y + row);
            for col in 0..w {
                out.push(self.at(v.0 + i64::from(x + col), cy));
            }
        }
        Some(out)
    }

    /// Runs THE walk over V = R ∪ canvas and folds its window back into the
    /// plane: the new stored rect and its samples, `None` when no written
    /// sample changed. `hole` is the request's (`edit.hole()`) for a mask or
    /// channel; a colour plane builds its own.
    pub(crate) fn transform(
        &self,
        edit: &SelectedPixelsEdit,
        canvas: (u32, u32),
        coverage: &[u16],
        landing: &Landing,
        hole: Hole,
        job: JobContext<'_>,
    ) -> Result<Option<(CanvasRect, Vec<S>)>, String> {
        let v = union(self.rect, (0, 0, canvas.0, canvas.1));
        let moved = move_samples::<S>(
            |x, y, w, h| self.read(v, x, y, w, h),
            v,
            canvas,
            coverage,
            landing,
            edit.empty_area(),
            hole,
            edit.bounds(),
            job,
        )?;
        // V ⊇ canvas ⊇ landing, so the extent never grows past V.
        let MovedSamples::Rect((wx, wy, ww, wh), samples) = moved else {
            return Err(NO_PLANE.into());
        };
        let window = (v.0 + i64::from(wx), v.1 + i64::from(wy), ww, wh);
        let target = if self.grow {
            union(self.rect, window)
        } else {
            self.rect
        };
        let Some(written) = intersect(window, target) else {
            return Ok(None);
        };
        let from_window = |x: i64, y: i64| -> S {
            samples[(y - window.1) as usize * window.2 as usize + (x - window.0) as usize]
        };
        let mut changed = false;
        for row in 0..written.3 {
            job.check()?;
            let y = written.1 + i64::from(row);
            changed = (0..written.2).any(|col| {
                let x = written.0 + i64::from(col);
                from_window(x, y) != self.at(x, y)
            });
            if changed {
                break;
            }
        }
        if !changed {
            return Ok(None);
        }
        if area(target) > crate::doc::MAX_PIXELS {
            return Err("the transformed plane exceeds the supported pixel limit".into());
        }
        let mut out = reserved_vec(area(target) as usize)?;
        for row in 0..target.3 {
            job.check()?;
            let y = target.1 + i64::from(row);
            for col in 0..target.2 {
                let x = target.0 + i64::from(col);
                out.push(if contains(window, (x, y)) {
                    from_window(x, y)
                } else {
                    self.at(x, y)
                });
            }
        }
        Ok(Some((target, out)))
    }
}

/// `plan_scratch` + `landing_scratch` (or `bounded_scratch`) over V, plus the
/// written plane at `sample_bytes` per pixel of V.
pub(crate) fn plane_scratch(
    edit: &SelectedPixelsEdit,
    canvas: (u32, u32),
    rect: CanvasRect,
    sample_bytes: u64,
) -> Result<u64, String> {
    let v = union(rect, (0, 0, canvas.0, canvas.1));
    let walk = match edit.bounds() {
        Some(bounds) => bounded_scratch(edit, canvas, v, bounds, sample_bytes)?,
        None => {
            plan_scratch(canvas, v, sample_bytes).saturating_add(edit.landing_scratch(canvas, v)?)
        }
    };
    Ok(walk.saturating_add(sample_bytes.saturating_mul(area(v))))
}

fn offset_of(rect: CanvasRect) -> Result<(i32, i32), String> {
    let overflow = || "the transformed mask's offset leaves i32".to_string();
    Ok((
        i32::try_from(rect.0).map_err(|_| overflow())?,
        i32::try_from(rect.1).map_err(|_| overflow())?,
    ))
}

impl DrDocument {
    fn selected_plane_source(&self, edit: &SelectedPixelsEdit) -> Result<Source<'_, u8>, String> {
        match edit.target() {
            SelectedTarget::Mask => {
                let index = edit.layer();
                let entry = self
                    .layers
                    .get(index)
                    .ok_or("selected pixels layer index is out of range")?;
                let mask = entry
                    .mask
                    .as_deref()
                    .ok_or_else(|| format!("layer {index} has no mask"))?;
                let (w, h) = mask.dimensions();
                let offset = entry.mask_properties.offset;
                Ok(Source {
                    rect: (i64::from(offset.0), i64::from(offset.1), w, h),
                    samples: mask.as_raw(),
                    outside: entry.mask_properties.outside_coverage,
                    grow: entry.kind != LayerKind::Group,
                })
            }
            SelectedTarget::Channel(i) => {
                let channel = self
                    .channels
                    .get(i)
                    .ok_or_else(|| format!("channel {i} is out of range"))?;
                Ok(Source {
                    rect: (0, 0, self.width, self.height),
                    samples: channel.data.as_raw(),
                    outside: 0,
                    grow: false,
                })
            }
            _ => Err(NO_PLANE.into()),
        }
    }

    /// The byte plan of a mask/channel target: sample-free; validates the
    /// target and a transform's geometry. No lock check.
    pub(crate) fn selected_plane_plan(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<SelectedPixelsPlan, String> {
        let source = self.selected_plane_source(edit)?;
        let canvas = (self.width, self.height);
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        Ok(SelectedPixelsPlan {
            retained_bytes: inventory.unique_bytes(),
            scratch_bytes: plane_scratch(edit, canvas, source.rect, 1)?,
            input_samples: u64::from(canvas.0) * u64::from(canvas.1),
        })
    }

    /// Transform the targeted mask's or channel's selected coverage. Refuses,
    /// in order: the coverage length, the target, a Lock All mask; then the
    /// identity is `Ok(None)`; then the plan's admission and reservation.
    /// `Ok(None)` when no stored sample changed.
    pub(crate) fn selected_plane_edit(
        &self,
        edit: &SelectedPixelsEdit,
        coverage: &[u16],
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let canvas = (self.width, self.height);
        if coverage.len() as u64 != u64::from(canvas.0) * u64::from(canvas.1) {
            return Err(COVERAGE_LENGTH.into());
        }
        let source = self.selected_plane_source(edit)?;
        if edit.target() == SelectedTarget::Mask {
            let index = edit.layer();
            let blocking = self.lock_block(index, EditKind::Mask);
            if blocking != 0 {
                return Err(match self.lock_owner(index, blocking) {
                    Some(owner) if owner != index => {
                        format!("layer {index} is inside group {owner}, which is locked (all)")
                    }
                    _ => format!("layer {index} is locked (all)"),
                });
            }
        }
        let Some(landing) = edit.landing(canvas)? else {
            return Ok(None);
        };
        let plan = self.selected_plane_plan(edit)?;
        admit(
            plan.retained_bytes,
            plan.scratch_bytes,
            other_retained,
            limit,
        )?;
        job.reserve(plan.scratch_bytes)?;
        job.check()?;
        let hole = edit.hole();
        let Some((rect, samples)) =
            source.transform(edit, canvas, coverage, &landing, hole, job)?
        else {
            return Ok(None);
        };
        let image = GrayImage::from_raw(rect.2, rect.3, samples).ok_or(NO_PLANE)?;
        let mut next = self.clone();
        match edit.target() {
            SelectedTarget::Mask => {
                let index = edit.layer();
                let old = &self.layers[index].mask_properties;
                let offset = offset_of(rect)?;
                let dims = (rect.2, rect.3);
                let fits = self.native_masks_fit_dimensions(
                    self.layers.iter().enumerate().filter_map(|(i, l)| {
                        if i == index {
                            Some(dims)
                        } else {
                            l.mask.as_ref().map(|m| m.dimensions())
                        }
                    }),
                );
                if !old.valid(dims) || !fits {
                    return Err("the transformed mask exceeds the native mask limit".into());
                }
                let layer = &mut next.layers[index];
                if offset != old.offset {
                    let mut properties = old.clone();
                    properties.offset = offset;
                    layer.mask_properties = properties;
                }
                layer.mask = Some(Arc::new(image));
            }
            SelectedTarget::Channel(i) => next.channels[i].data = Arc::new(image),
            _ => return Err(NO_PLANE.into()),
        }
        job.check()?;
        Ok(Some(next))
    }
}

impl PrecisionDocument {
    fn selected_plane_source(&self, edit: &SelectedPixelsEdit) -> Result<Source<'_, u16>, String> {
        match edit.target() {
            SelectedTarget::Mask => {
                let index = edit.layer();
                let record = self
                    .layer(index)
                    .ok_or("high-depth layer index is out of range")?;
                let mask = record
                    .mask()
                    .ok_or_else(|| format!("layer {index} has no mask"))?;
                let (w, h) = mask.plane().dimensions();
                let properties = mask.properties();
                let offset = properties.offset;
                Ok(Source {
                    rect: (i64::from(offset.0), i64::from(offset.1), w, h),
                    samples: mask.plane().samples(),
                    outside: u16::from(properties.outside_coverage) * 257,
                    grow: record.children().is_none(),
                })
            }
            SelectedTarget::Channel(i) => {
                let channel = self
                    .channels()
                    .get(i)
                    .ok_or_else(|| format!("channel {i} is out of range"))?;
                let (w, h) = self.dimensions();
                Ok(Source {
                    rect: (0, 0, w, h),
                    samples: channel.plane().samples(),
                    outside: 0,
                    grow: false,
                })
            }
            _ => Err(NO_PLANE.into()),
        }
    }

    /// The typed twin of `DrDocument::selected_plane_plan` (2 bytes per
    /// sample at 16 and 32 bits). Never refuses float storage.
    pub(crate) fn selected_plane_plan(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<SelectedPixelsPlan, String> {
        let source = self.selected_plane_source(edit)?;
        let canvas = self.dimensions();
        Ok(SelectedPixelsPlan {
            retained_bytes: self.retained_bytes(),
            scratch_bytes: plane_scratch(edit, canvas, source.rect, 2)?,
            input_samples: u64::from(canvas.0) * u64::from(canvas.1),
        })
    }

    /// The typed twin of `DrDocument::selected_plane_edit`, same refusals in
    /// the same order.
    pub(crate) fn selected_plane_with_context(
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
        let source = self.selected_plane_source(edit)?;
        if edit.target() == SelectedTarget::Mask {
            self.lock_refusal(edit.layer(), EditKind::Mask)?;
        }
        let Some(landing) = edit.landing(canvas)? else {
            return Ok(None);
        };
        let plan = self.selected_plane_plan(edit)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        job.check()?;
        let hole = edit.hole();
        let Some((rect, samples)) =
            source.transform(edit, canvas, coverage, &landing, hole, job)?
        else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let plane = PrecisionPlane::adopt(rect.2, rect.3, samples)?;
        let result = match edit.target() {
            SelectedTarget::Mask => {
                let index = edit.layer();
                let mask = self
                    .layer(index)
                    .and_then(|record| record.mask())
                    .ok_or_else(|| format!("layer {index} has no mask"))?;
                let mut properties = mask.properties().clone();
                properties.offset = offset_of(rect)?;
                let next = PrecisionMask::new(plane, properties, mask.enabled())?;
                self.set_mask_with_context(index, Some(next), job)?
            }
            SelectedTarget::Channel(i) => {
                let old = &self.channels()[i];
                let mut channels = self.channels().to_vec();
                let mut channel = PrecisionChannel::new(old.name(), plane)?;
                channel.id = old.id;
                channel.metadata = old.metadata.clone();
                channels[i] = channel;
                Some(self.rebuilt(
                    self.width,
                    self.height,
                    self.layers.as_ref().clone(),
                    channels,
                )?)
            }
            _ => return Err(NO_PLANE.into()),
        };
        job.report(1.0, 5)?;
        Ok(result)
    }
}
