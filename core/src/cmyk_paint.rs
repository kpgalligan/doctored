//! Painting in ink (#547 phase 2): the Brush, Eraser, Fill (`paint_blend`),
//! Gradient and Paint Bucket on a CMYK document's raster records —
//! [`CmykPaintEdit`], the CMYK twin of `precision_paint_tools`'
//! `NativePaintEdit`, parsed by `dr_precision_doc_edit` on a CMYK document.
//!
//! The request is `NativePaintEdit`'s with ink in place of colour: `ink` (and
//! each gradient stop's `ink`) is straight `[c, m, y, k, alpha]` in 0…1, ink 1
//! being 100 %. The input planes and the plan's JSON keys are that edit's
//! exactly: plane 0 the canvas stroke coverage, plane 1 the selection when
//! `selection`. A `blend_mode` outside `CMYK_BLEND_MODES` (Pass Through, a
//! group's), a `plane` target and an RGB `color`/`color_space` refuse by
//! name; Dodge/Burn is `NativePaintEdit`'s (`cmyk_retouch_brush`, #547).
//!
//! Per pixel, with `s` the coverage (stroke × selection) and the walk over
//! the canvas rect the coverage can change:
//!
//! * paint, fill, gradient, bucket: straight CMYKA over at
//!   `sa = ink_alpha × opacity × s` through the request's blend mode on
//!   `1 − ink` (`cmyk_composite::over_mode`, whose Normal is `over`, master
//!   `blend::composite_source_into`); under Preserve Transparency or a
//!   transparency lock the backdrop's alpha is frozen
//!   (`cmyk_composite::over_mode_frozen`, master
//!   `blend::composite_source_frozen`) and nothing grows;
//! * erase: alpha through `blend::erase_alpha` at `s × opacity`, ink
//!   untouched; a transparency-locked layer erases nothing (its alpha is
//!   frozen, as the RGB restore leaves it);
//! * gradient: the stops' straight CMYKA between the two that bracket `t`
//!   (`gradient_field::segment`; the inks through the Classic ramp blended
//!   with the linear one by the request's `smoothness`, a fraction defaulting
//!   to 1, `gradient_field::smooth_color`), `t` the drag's styled
//!   parameter (`gradient_field::styled_parameter`, absent style = linear)
//!   at the pixel centre;
//! * bucket: the region is `selection_region::grow_ink` from the seed over
//!   the target layer's samples (outside it, bare paper) at the document's
//!   depth: the CMYK wand's predicate through the one `flood`
//!   (`selection_region`'s module doc, **Ink**).
//!
//! Growth, clipping, the no-op and the locks are `precision_paint_tools`'
//! rules: changed ink may grow the layer to the union of its rect and the
//! changed pixels (`retouch_transform::merged_extent`), an unchanged walk is
//! `Ok(None)`, and the body runs under a `Pixels` lock edit.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::blend::BlendMode;
use crate::cmyk_composite::{code, codes, over_mode, over_mode_frozen, unit, InkPixel};
use crate::cmyk_document::refused_sentence;
use crate::cmyk_image::{CmykImage, CmykSample, PAPER};
use crate::doc_lock::LOCK_TRANSPARENCY;
use crate::gradient_field::{FieldStop, FieldStyle};
use crate::job_control::JobContext;
use crate::layer_rect::{canvas_rect_on_plane, coverage_rect16, plane_rect_offset};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::precision_paint::canvas_index;
use crate::retouch_transform::{merged_extent, StorageExtent};

/// A canvas pixel's charge beside the layer planes: the walk's CMYKA16 rect
/// (10 B), the host's coverage and selection planes (4 B) and the host's
/// display preparation of the result (the view's render and its display
/// conversion). Measured by `scripts/cmyk-admission/measure.py`
/// (`fill-16`, `gradient-16`, `bucket-16`); a miss raises this term.
pub(crate) const CANVAS_BYTES_PER_PIXEL: u64 = 32;

/// One stop of a CMYK gradient: its position along the ramp, 0…1, its
/// straight `[c, m, y, k, alpha]` ink, and its ink and alpha midpoints
/// toward the next stop (`gradient_field::FieldStop`'s; 0.5 when
/// absent).
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CmykStop {
    pub position: f32,
    pub ink: [f32; 5],
    #[serde(default = "crate::gradient_field::half")]
    pub midpoint: f32,
    #[serde(default = "crate::gradient_field::half")]
    pub opacity_midpoint: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CmykPaintEdit {
    PaintBlend {
        layer: usize,
        ink: [f32; 5],
        opacity: f32,
        #[serde(default)]
        blend_mode: u32,
        #[serde(default)]
        selection: bool,
        #[serde(default)]
        preserve_alpha: bool,
    },
    BucketFill {
        layer: usize,
        ink: [f32; 5],
        opacity: f32,
        #[serde(default)]
        blend_mode: u32,
        x: i32,
        y: i32,
        tolerance: u16,
        contiguous: bool,
        #[serde(default)]
        selection: bool,
    },
    Gradient {
        layer: usize,
        stops: Vec<CmykStop>,
        opacity: f32,
        #[serde(default)]
        blend_mode: u32,
        from: [f64; 2],
        to: [f64; 2],
        #[serde(default)]
        style: Option<FieldStyle>,
        #[serde(default = "crate::gradient_field::full_smoothness")]
        smoothness: f32,
        #[serde(default)]
        selection: bool,
    },
    EraseNative {
        layer: usize,
        opacity: f32,
        #[serde(default)]
        selection: bool,
    },
}

/// The straight ink of `stops` at `t` under a `style` gradient at the
/// Smoothness `k` (a fraction): the four inks follow the Classic ramp blended
/// with the linear one by `k` (`gradient_field::smooth_color`, unbounded:
/// the bracket bound is the Gradient Map's, `cmyk_adjust_ink`; at `k` 1
/// `classic_color` exactly) where `FieldStyle::classic`, else linear between the bracketing stops
/// (`gradient_field::segment`, the ONE bracket search); alpha (`[4]`) is
/// linear in both; the inks read the fraction remapped through stop `i`'s
/// `midpoint`, alpha through its `opacity_midpoint`
/// (`gradient_field::midpoint_fraction`); the end stop's ink at or past
/// an end. `gradient_field::stop_color` is the master copy of this rule,
/// over four components; this is it over five.
fn stop_ink(stops: &[CmykStop], t: f32, style: FieldStyle, k: f32) -> [f32; 5] {
    let Some((i, j, f)) = crate::gradient_field::segment(stops.len(), t, |i| stops[i].position)
    else {
        return [0.0; 5];
    };
    let a = stops[i].ink;
    if i == j {
        return a;
    }
    let b = stops[j].ink;
    let fi = crate::gradient_field::midpoint_fraction(f, stops[i].midpoint);
    let fa = crate::gradient_field::midpoint_fraction(f, stops[i].opacity_midpoint);
    let mut out: [f32; 5] = std::array::from_fn(|c| {
        let g = if c == 4 { fa } else { fi };
        a[c] + (b[c] - a[c]) * g
    });
    if style.classic() {
        let inks = crate::gradient_field::smooth_color::<4>(
            stops.len(),
            t,
            k,
            crate::gradient_field::ClassicBound::Unbounded,
            |s| stops[s].position,
            |s| stops[s].midpoint,
            |s| {
                let ink = stops[s].ink;
                [ink[0], ink[1], ink[2], ink[3]]
            },
        );
        out[..4].copy_from_slice(&inks);
    }
    out
}

impl CmykPaintEdit {
    /// Parses a request (module doc), refusing by name what the CMYK mode
    /// does not paint.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("invalid high-depth edit request: {e}"))?;
        let object = value
            .as_object()
            .ok_or("a CMYK paint request must be a JSON object")?;
        if object.contains_key("plane") {
            return Err(refused_sentence("Painting one colour channel"));
        }
        if ["color", "color_space", "start", "end", "radial"]
            .iter()
            .any(|key| object.contains_key(*key))
        {
            return Err(
                "a CMYK document paints in ink: pass ink (or gradient stops with ink) as \
                 [c, m, y, k, alpha] in 0…1, not an RGB color"
                    .into(),
            );
        }
        let edit: Self =
            serde_json::from_value(value).map_err(|e| format!("invalid CMYK paint tool: {e}"))?;
        if let Some(mode) = BlendMode::from_c(edit.blend_mode() as i32) {
            if let Some(refusal) = crate::cmyk_document::blend_refusal(mode) {
                return Err(refusal);
            }
        }
        Ok(edit)
    }

    /// The request's blend mode (`plan` refuses an unknown value).
    fn mode(&self) -> BlendMode {
        BlendMode::from_c(self.blend_mode() as i32).unwrap_or(BlendMode::Normal)
    }

    fn blend_mode(&self) -> u32 {
        match *self {
            Self::PaintBlend { blend_mode, .. }
            | Self::BucketFill { blend_mode, .. }
            | Self::Gradient { blend_mode, .. } => blend_mode,
            Self::EraseNative { .. } => 0,
        }
    }

    /// The target record, the opacity and whether plane 1 is a selection.
    fn target(&self) -> (usize, f32, bool) {
        match *self {
            Self::PaintBlend {
                layer,
                opacity,
                selection,
                ..
            }
            | Self::BucketFill {
                layer,
                opacity,
                selection,
                ..
            }
            | Self::Gradient {
                layer,
                opacity,
                selection,
                ..
            }
            | Self::EraseNative {
                layer,
                opacity,
                selection,
            } => (layer, opacity, selection),
        }
    }

    fn preserves_alpha(&self) -> bool {
        matches!(
            self,
            Self::PaintBlend {
                preserve_alpha: true,
                ..
            }
        )
    }

    /// Whether changed ink may grow the layer: never for the Eraser, under
    /// Preserve Transparency or on a transparency-locked layer.
    fn expands(&self, doc: &PrecisionDocument) -> bool {
        !matches!(self, Self::EraseNative { .. })
            && !self.preserves_alpha()
            && doc.lock_owner(self.target().0, LOCK_TRANSPARENCY).is_none()
    }

    /// The target's image and offset; refuses a record that is not a CMYK
    /// raster.
    fn layer<'a>(&self, doc: &'a PrecisionDocument) -> Result<(&'a CmykImage, (i32, i32)), String> {
        let layer = doc
            .layer(self.target().0)
            .ok_or("CMYK paint target is out of range")?;
        let image = layer
            .cmyk_source()
            .ok_or("CMYK paint requires a CMYK raster layer")?;
        Ok((image, layer.offset()))
    }

    /// The layer's storage extent after the walk.
    fn extent(&self, doc: &PrecisionDocument) -> Result<StorageExtent, String> {
        let (image, offset) = self.layer(doc)?;
        if self.expands(doc) {
            merged_extent(
                (image.dimensions(), offset),
                ((doc.width, doc.height), (0, 0)),
            )
        } else {
            Ok((image.dimensions(), offset))
        }
    }

    fn inks(&self) -> Vec<[f32; 5]> {
        match self {
            Self::PaintBlend { ink, .. } | Self::BucketFill { ink, .. } => vec![*ink],
            Self::Gradient { stops, .. } => stops.iter().map(|stop| stop.ink).collect(),
            Self::EraseNative { .. } => Vec::new(),
        }
    }

    /// The plan: `NativePaintEdit`'s keys (module doc).
    pub(crate) fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        doc.require_cmyk_policy("CMYK paint")?;
        let (index, opacity, selection) = self.target();
        if !opacity.is_finite()
            || !(0. ..=1.).contains(&opacity)
            || BlendMode::from_c(self.blend_mode() as i32).is_none()
        {
            return Err("native paint opacity or blend mode is invalid".into());
        }
        if self
            .inks()
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("CMYK ink must be finite in [0,1]".into());
        }
        match self {
            Self::Gradient {
                from,
                to,
                stops,
                smoothness,
                ..
            } => {
                crate::gradient_field::validate(*from, *to)?;
                crate::gradient_field::validate_smoothness(*smoothness)?;
                if !crate::style_json::GRADIENT_STOPS.contains(&stops.len()) {
                    return Err("native gradient stops must have 2 to 32 entries".into());
                }
                // The positions' rules are the RGB stops' (the ink is checked
                // above): the shared validator over position and alpha.
                let positions: Vec<FieldStop> = stops
                    .iter()
                    .map(|stop| FieldStop {
                        position: stop.position,
                        color: [0.0, 0.0, 0.0, stop.ink[4]],
                        midpoint: stop.midpoint,
                        opacity_midpoint: stop.opacity_midpoint,
                    })
                    .collect();
                crate::gradient_field::validate_stops(&positions)?;
            }
            Self::BucketFill { x, y, .. }
                if canvas_index(i64::from(*x), i64::from(*y), doc.width, doc.height).is_none() =>
            {
                return Err("native bucket seed is outside canvas".into())
            }
            _ => {}
        }
        let (image, _) = self.layer(doc)?;
        let extent = self.extent(doc)?;
        let n = pixel_count(doc.width, doc.height)? as u64;
        let output = pixel_count(extent.0 .0, extent.0 .1)? as u64;
        let (lw, lh) = image.dimensions();
        let mut scratch = 8 * BOOKKEEPING_BYTES + n * CANVAS_BYTES_PER_PIXEL + output * 10;
        if matches!(self, Self::BucketFill { .. }) {
            // The layer's contiguous copy the flood reads and the region.
            scratch += pixel_count(lw, lh)? as u64 * 10 + n * 2;
        }
        let (mut rasters, masks) = doc.native_dimensions();
        let raster = (0..index)
            .filter(|&i| {
                doc.layer(i)
                    .and_then(PrecisionDocumentLayer::cmyk_source)
                    .is_some()
            })
            .count();
        if let Some(slot) = rasters.get_mut(raster) {
            *slot = extent.0;
        }
        doc.native_fits_geometry(doc.dimensions(), &rasters, &masks, doc.channels.len())?;
        let planes: u64 = if selection { 2 } else { 1 };
        Ok(
            json!({"retained_bytes":doc.retained_bytes(),"scratch_bytes":scratch,
            "input_samples":n*planes,"input_width":doc.width,"input_height":doc.height,
            "input_planes":planes}),
        )
    }

    /// Applies the request under the target's locks (`EditKind::Pixels`).
    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let (index, ..) = self.target();
        doc.under_locks(index, crate::doc_lock::EditKind::Pixels, |doc| {
            self.applying_unlocked(doc, input, limit, other, job)
        })
    }

    fn applying_unlocked(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let plan = self.plan(doc)?;
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .ok_or("invalid CMYK paint plan")?;
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        if input.len() as u64 != plan["input_samples"].as_u64().unwrap_or(0) {
            return Err("native paint input length differs from plan".into());
        }
        let (index, opacity, selected) = self.target();
        let frozen = self.preserves_alpha() || doc.lock_owner(index, LOCK_TRANSPARENCY).is_some();
        if opacity == 0. || (frozen && matches!(self, Self::EraseNative { .. })) {
            job.report(1., 5)?;
            return Ok(None);
        }
        let n = pixel_count(doc.width, doc.height)?;
        let canvas = (doc.width, doc.height);
        if let Self::BucketFill { x, y, .. } = *self {
            let seed = y as usize * doc.width as usize + x as usize;
            if input[seed] == 0 || (selected && input[n + seed] == 0) {
                job.report(1., 5)?;
                return Ok(None);
            }
        }
        let (image, (ox, oy)) = self.layer(doc)?;
        let (w, h) = image.dimensions();
        let expands = self.expands(doc);
        let region = if let Self::BucketFill {
            x,
            y,
            tolerance,
            contiguous,
            ..
        } = *self
        {
            // The flood reads the layer once, as one contiguous copy.
            let before = image.to_contiguous()?;
            let sample = |i: usize| -> CmykSample {
                let cx = (i % doc.width as usize) as i64;
                let cy = (i / doc.width as usize) as i64;
                canvas_index(cx - i64::from(ox), cy - i64::from(oy), w, h)
                    .map_or(PAPER, |j| before[j])
            };
            let depth_bits = doc
                .require_cmyk_policy("The CMYK Paint Bucket")?
                .depth
                .bits();
            Some(crate::selection_region::grow_ink(
                canvas,
                sample,
                (x as u32, y as u32),
                tolerance,
                contiguous,
                1,
                depth_bits,
                job.range(0., 0.3),
            )?)
        } else {
            None
        };
        // The walk covers only the canvas rect the coverage can change
        // (clipped to the layer when nothing may grow).
        let walk = if expands {
            coverage_rect16(&input[..n], canvas, canvas, (0, 0))
        } else {
            coverage_rect16(&input[..n], canvas, (w, h), (ox, oy)).map(|rect| {
                let (x, y) = plane_rect_offset((ox, oy), rect);
                (x as u32, y as u32, rect.2, rect.3)
            })
        };
        let Some((rx, ry, rw, rh)) = walk else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let area = rw as usize * rh as usize;
        let mut rect = reserved_vec(area)?;
        rect.resize(area, PAPER);
        let ends = |a: u32, b: u32| (i64::from(a), i64::from(a) + i64::from(b));
        let ((x0, x1), (y0, y1)) = (ends(rx, rw), ends(ry, rh));
        let on_layer = canvas_rect_on_plane(canvas, (w, h), (ox, oy), (x0, y0, x1, y1));
        if let Some(part) = on_layer {
            let samples = image.read_rect(part, job)?;
            let (cx, cy) = plane_rect_offset((ox, oy), part);
            let (dx, dy) = ((cx as u32 - rx) as usize, (cy as u32 - ry) as usize);
            for (row, line) in samples.chunks_exact(part.2 as usize).enumerate() {
                let at = (dy + row) * rw as usize + dx;
                rect[at..at + line.len()].copy_from_slice(line);
            }
        }
        let mut bounds = [doc.width, doc.height, 0, 0];
        let mut changed = false;
        for (k, slot) in rect.iter_mut().enumerate() {
            job.every(k)?;
            if k & 65535 == 0 {
                job.report(0.3 + 0.5 * k as f64 / area as f64, 3)?;
            }
            let x = rx + (k % rw as usize) as u32;
            let y = ry + (k / rw as usize) as u32;
            let i = y as usize * doc.width as usize + x as usize;
            let original = *slot;
            let coverage = f64::from(input[i]) / 65535.
                * if selected {
                    f64::from(input[n + i]) / 65535.
                } else {
                    1.
                };
            if coverage <= 0. || region.as_ref().is_some_and(|r| r[i] == 0) {
                continue;
            }
            let result = self.pixel(
                original,
                coverage,
                opacity,
                frozen,
                (i64::from(x), i64::from(y)),
            );
            if result != original {
                *slot = result;
                changed = true;
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
        }
        if !changed {
            job.report(1., 5)?;
            return Ok(None);
        }
        let extent = if expands {
            merged_extent(
                ((w, h), (ox, oy)),
                (
                    (bounds[2] - bounds[0], bounds[3] - bounds[1]),
                    (bounds[0] as i32, bounds[1] as i32),
                ),
            )?
        } else {
            ((w, h), (ox, oy))
        };
        let ((nw, nh), (nx, ny)) = extent;
        let out = match on_layer {
            Some(part) if extent == ((w, h), (ox, oy)) => {
                // R ∩ layer back through `with_rect` (untouched tiles shared);
                // its changed latch is the bitwise diff.
                let samples = if (part.2, part.3) == (rw, rh) {
                    rect
                } else {
                    let (cx, cy) = plane_rect_offset((ox, oy), part);
                    let (dx, dy) = ((cx as u32 - rx) as usize, (cy as u32 - ry) as usize);
                    let mut samples = reserved_vec(part.2 as usize * part.3 as usize)?;
                    for row in 0..part.3 as usize {
                        let at = (dy + row) * rw as usize + dx;
                        samples.extend_from_slice(&rect[at..at + part.2 as usize]);
                    }
                    samples
                };
                let Some(out) = image.with_rect(part, &samples, job)? else {
                    job.report(1., 5)?;
                    return Ok(None);
                };
                out
            }
            _ => {
                // The grown extent: bare paper, the old layer's rows, then
                // the walk's samples over the rect ∩ extent.
                let (left, top) = (i64::from(nx), i64::from(ny));
                let (r0, r1) = (x0.max(left), x1.min(left + i64::from(nw)));
                CmykImage::from_rows_with_job(
                    nw,
                    nh,
                    |y, row| {
                        row.fill(PAPER);
                        let cy = i64::from(y) + top;
                        let ly = cy - i64::from(oy);
                        if (0..i64::from(h)).contains(&ly) {
                            let at = (i64::from(ox) - left) as usize;
                            for (x, run) in image.row_segments(ly as u32, 0, w) {
                                let at = at + x as usize;
                                row[at..at + run.len()].copy_from_slice(run);
                            }
                        }
                        if (y0..y1).contains(&cy) && r0 < r1 {
                            let from = (cy - y0) as usize * rw as usize + (r0 - x0) as usize;
                            let at = (r0 - left) as usize;
                            let len = (r1 - r0) as usize;
                            row[at..at + len].copy_from_slice(&rect[from..from + len]);
                        }
                        job.check()
                    },
                    job,
                )?
            }
        };
        job.check()?;
        let mut next = doc.replacing_content(
            index,
            Content::CmykRaster {
                image: out,
                offset: (nx, ny),
            },
        )?;
        if let Some(next) = next.as_mut() {
            if let Some(comps) = doc.comps_after_growth(index, (nx, ny)) {
                next.comps = comps;
            }
        }
        job.report(1., 5)?;
        Ok(next)
    }

    /// One pixel of the walk (module doc) at coverage `s`, canvas pixel
    /// `(x, y)` (the gradient's centre and Dissolve's dither).
    pub(crate) fn pixel(
        &self,
        original: CmykSample,
        s: f64,
        opacity: f32,
        frozen: bool,
        (x, y): (i64, i64),
    ) -> CmykSample {
        if let Self::EraseNative { .. } = self {
            let mut out = original;
            out[4] = code(crate::blend::erase_alpha(
                unit(original[4]),
                (s * f64::from(opacity)) as f32,
            ));
            return out;
        }
        let ink = match self {
            Self::Gradient {
                stops,
                from,
                to,
                style,
                smoothness,
                ..
            } => {
                let point = [x as f64 + 0.5, y as f64 + 0.5];
                let style = style.unwrap_or(FieldStyle::Linear);
                let t = crate::gradient_field::styled_parameter(point, *from, *to, style);
                stop_ink(stops, t as f32, style, *smoothness)
            }
            Self::PaintBlend { ink, .. } | Self::BucketFill { ink, .. } => *ink,
            Self::EraseNative { .. } => return original,
        };
        let sa = (f64::from(ink[4]) * f64::from(opacity) * s) as f32;
        if sa <= 0. {
            return original;
        }
        let mut pixel: InkPixel = original.map(unit);
        let source = [ink[0], ink[1], ink[2], ink[3]];
        let xy = (x, y);
        if frozen {
            over_mode_frozen(&mut pixel, source, sa, self.mode(), xy);
        } else {
            over_mode(&mut pixel, source, sa, self.mode(), xy);
        }
        let result = codes(pixel);
        // Sub-quantum alpha cannot create invisible storage outside the layer.
        if original == PAPER && result[4] == 0 {
            return original;
        }
        result
    }
}
