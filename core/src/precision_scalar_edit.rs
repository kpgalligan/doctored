//! Native mask/channel/RGBA-plane edits. Source neighborhoods use complete raw
//! storage; only final scalar writeback is gated in canvas coordinates. Other
//! color components, independent mask placement and channel identity stay exact.
//!
//! Mask/channel/RGBA writes preserve raw placement, unchanged components and metadata; canvas
//! coverage gates the final scalar result once. Quick Mask shares one integer scalar source-over.
//! Scalar ink uses the single Rec.709 constants. All requests plan input/output/live scratch before
//! allocation, stage publication and use single-use cancellation; ordinary radius cap250 has an
//! explicit work forecast. Native channel conveniences also use `precision_plane_math.rs` (the
//! existing plane-blend table and nine luminosity expressions), `precision_mask_apply.rs`
//! (effective mask into alpha with exact hidden RGB), independent scalar Auto derivation with raw
//! mapped analysis coverage, and `replace_canvas` for mapped Apply Image writeback. Plane RGB
//! inputs/outputs are planar; the four HSL operators require true triples.
//! `precision_channel_math.rs` qualifies independent W3C HSL/positive-sum blends, luminosity
//! powers, Auto endpoints and mask application.
//!
//! `dodge_burn` (R4m of `display_encoding`) is Dodge or Burn on a mask or alpha channel
//! at every native depth (the plane is u16 at 16 and 32 bits): the sample becomes
//! `65535·doc_retouch::shaped(s/65535)`, lerped by stroke × selection coverage. A mask is
//! coverage, not light, so nothing is encoded. Its payload is the canvas stroke coverage, as
//! `paint`'s is; exposure 0 is `Ok(None)`.
//!
//! A filter in a 32-bit float RGB document on a mask, a channel or one float
//! Red/Green/Blue/Alpha plane runs the float route under `float_plane_filter`'s plane rule.
use crate::gradient_field::{FieldStop, FieldStyle};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionRun, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_mask::PrecisionPlane;
use crate::precision_selection_plane::{copy, PlaneOperation};
use crate::selected_adjustment::{coverage_at, AnalysisScope, SelectedEdit};
use crate::tile_store::TILE_EDGE;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
type PlaneGeometry = ((u32, u32), (i32, i32));
fn yes() -> bool {
    true
}
fn one() -> f64 {
    1.
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalarTarget {
    Mask,
    Channel,
    Red,
    Green,
    Blue,
    Alpha,
    /// One ink plate of a CMYK raster, as its stored `1 − ink`
    /// (`cmyk_plate_scalar`).
    Cyan,
    Magenta,
    Yellow,
    Black,
    /// A Grayscale document's layer (#547 phase 6b): its one colour
    /// channel, read from component 0 and written to R, G and B so the
    /// layer keeps R = G = B. Apply Image's `replace_canvas` write only.
    Gray,
}
impl ScalarTarget {
    /// The CMYK plate a plate target names.
    fn plate(self) -> Option<usize> {
        match self {
            Self::Cyan => Some(0),
            Self::Magenta => Some(1),
            Self::Yellow => Some(2),
            Self::Black => Some(3),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarEdit {
    op: String,
    pub target: ScalarTarget,
    pub index: usize,
    #[serde(default)]
    selection: bool,
    operation: ScalarOperation,
    /// Whether this is Invert on a layer mask ([`mask_invert_request`]), set
    /// by [`ScalarEdit::parse`]: the one scalar edit that follows the
    /// whole-canvas rule of `doc_mask_invert`.
    #[serde(skip)]
    mask_invert: bool,
}
/// Whether `edit` is the Invert adjustment, the request the host sends for
/// Image ▸ Adjustments ▸ Invert on a scalar target: `op` `adjustment` with a
/// description whose own `op` is `invert`.
fn mask_invert_request(edit: &SelectedEdit) -> bool {
    edit.op == "adjustment"
        && edit
            .description
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .is_some_and(|value| value["op"] == "invert")
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScalarOperation {
    Fill {
        value: u16,
        #[serde(default = "one")]
        opacity: f64,
        /// A blend mode code (`BlendMode::from_c`); Normal (0) is the lerp.
        #[serde(default)]
        blend_mode: i32,
    },
    Paint {
        value: u16,
        #[serde(default = "one")]
        opacity: f64,
        #[serde(default)]
        blend_mode: i32,
    },
    Gradient {
        from: [f64; 2],
        to: [f64; 2],
        radial: bool,
        start: u16,
        end: u16,
        #[serde(default = "one")]
        start_alpha: f64,
        #[serde(default = "one")]
        end_alpha: f64,
        #[serde(default = "one")]
        opacity: f64,
        /// A stops gradient: 2..32 stops `[g, g, g, alpha]`, `g` the
        /// gray over 65535. Empty is the two-endpoint gradient above, whose
        /// bytes are unchanged; with stops, `start`/`end` stay required and
        /// carry the first and last stop.
        #[serde(default)]
        stops: Vec<FieldStop>,
        /// The stops gradient's shape; absent is `radial`'s. Only with stops.
        #[serde(default)]
        style: Option<FieldStyle>,
        /// Smoothness as a fraction (`gradient_field::smooth_color`): 1
        /// (absent) is the Classic ramp exactly; below 1 only with stops.
        #[serde(default = "crate::gradient_field::full_smoothness")]
        smoothness: f32,
    },
    BucketFill {
        x: i32,
        y: i32,
        tolerance: u16,
        #[serde(default = "yes")]
        contiguous: bool,
        value: u16,
        #[serde(default = "one")]
        opacity: f64,
    },
    /// Dodge or Burn (R4m; module doc): `range` 0 shadows, 1
    /// midtones, 2 highlights; the payload is the canvas stroke coverage.
    DodgeBurn {
        exposure: f32,
        range: u8,
        burn: bool,
        /// Protect Tones (#547), accepted so the host sends one request at
        /// every target: a single plane is a group of one, whose protected
        /// curve is the plain one (`doc_retouch::protected`), so it changes
        /// nothing here.
        #[serde(default)]
        #[allow(dead_code)]
        protect_tones: bool,
    },
    Replace {},
    ReplaceCanvas {},
    Filter {
        edit: SelectedEdit,
    },
    Morphology {
        operation: PlaneOperation,
    },
}
impl ScalarEdit {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut edit: Self =
            serde_json::from_str(text).map_err(|e| format!("invalid native scalar edit: {e}"))?;
        if edit.op != "scalar_edit" {
            return Err("expected scalar_edit request".into());
        }
        edit.mask_invert = matches!(edit.target, ScalarTarget::Mask)
            && matches!(&edit.operation, ScalarOperation::Filter { edit } if mask_invert_request(edit));
        match &edit.operation {
            ScalarOperation::Fill { opacity, .. }
            | ScalarOperation::Paint { opacity, .. }
            | ScalarOperation::Gradient { opacity, .. }
            | ScalarOperation::BucketFill { opacity, .. }
                if !opacity.is_finite() || !(0. ..=1.).contains(opacity) =>
            {
                return Err("scalar opacity must be finite in [0,1]".into())
            }
            ScalarOperation::Filter { edit } => {
                SelectedEdit::parse(&serde_json::to_string(&json!({"layer":edit.layer,"op":edit.op,"params":edit.params,"description":edit.description,"analysis_scope":if edit.analysis_scope==AnalysisScope::Selection{"selection"}else{"whole_target"}})).unwrap())?;
                if edit.layer != 0
                    || (!edit.is_auto() && edit.analysis_scope != AnalysisScope::WholeTarget)
                {
                    return Err(
                        "scalar filter edit uses layer0; selection analysis belongs to Auto".into(),
                    );
                }
            }
            ScalarOperation::DodgeBurn {
                exposure, range, ..
            } if !exposure.is_finite() || !(0. ..=1.).contains(exposure) || *range > 2 => {
                return Err("native dodge/burn exposure or range is invalid".into())
            }
            ScalarOperation::Morphology { operation } if operation.planes() != 1 => {
                return Err("scalar morphology requires a one-plane operation".into())
            }
            // A selection-plane gradient is laid out in canvas coordinates; a
            // scalar target's gradient is the `gradient` operation, which maps
            // through the target's offset.
            ScalarOperation::Morphology {
                operation: PlaneOperation::Gradient { .. },
            } => {
                return Err("scalar morphology has no gradient; use the gradient operation".into())
            }
            _ => {}
        }
        if let ScalarOperation::Fill {
            blend_mode,
            opacity,
            ..
        }
        | ScalarOperation::Paint {
            blend_mode,
            opacity,
            ..
        } = edit.operation
        {
            crate::precision_plane_math::validate(1, blend_mode, opacity)?;
        }
        if let ScalarOperation::Gradient {
            from,
            to,
            start_alpha,
            end_alpha,
            stops,
            style,
            smoothness,
            ..
        } = &edit.operation
        {
            crate::gradient_field::validate(*from, *to)?;
            crate::gradient_field::validate_smoothness(*smoothness)?;
            if [start_alpha, end_alpha]
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(*v))
            {
                return Err("scalar gradient endpoint alpha must be finite in [0,1]".into());
            }
            if stops.is_empty() {
                if style.is_some() {
                    return Err("scalar gradient style requires stops".into());
                }
                if *smoothness < 1.0 {
                    return Err("scalar gradient smoothness requires stops".into());
                }
            } else {
                if !crate::style_json::GRADIENT_STOPS.contains(&stops.len()) {
                    return Err("scalar gradient stops must have 2 to 32 entries".into());
                }
                crate::gradient_field::validate_stops(stops)?;
                if stops
                    .iter()
                    .any(|s| s.color.iter().any(|v| !(0. ..=1.).contains(v)))
                {
                    return Err("scalar gradient stop values must be in [0,1]".into());
                }
            }
        }
        Ok(edit)
    }
    fn geometry(&self, doc: &PrecisionDocument) -> Result<PlaneGeometry, String> {
        if matches!(self.target, ScalarTarget::Gray) {
            if !doc.is_gray() {
                return Err("scalar target gray is a Grayscale document's layer".into());
            }
            if !matches!(self.operation, ScalarOperation::ReplaceCanvas {}) {
                return Err("scalar target gray takes replace_canvas only".into());
            }
        }
        match self.target {
            ScalarTarget::Channel => Ok((
                doc.channels()
                    .get(self.index)
                    .ok_or("native channel index is out of range")?
                    .plane()
                    .dimensions(),
                (0, 0),
            )),
            ScalarTarget::Mask => {
                let layer = doc.layer(self.index);
                let mask = layer
                    .and_then(|l| l.mask())
                    .ok_or("native scalar target has no mask")?;
                let stored = (mask.plane().dimensions(), mask.properties().offset);
                // A selection-scoped mask Invert writes over the union of the
                // stored rect and the canvas (`mask_invert`); a group's mask
                // keeps its rect, as at byte depth.
                let group = layer.is_some_and(|l| {
                    matches!(l.content, Content::Group(_) | Content::PassThrough(_))
                });
                if self.mask_invert && self.selection && !group {
                    return crate::mask_invert::union_with_canvas(stored, doc.dimensions())
                        .ok_or_else(|| "native mask invert extent is out of range".to_string());
                }
                Ok(stored)
            }
            ScalarTarget::Cyan
            | ScalarTarget::Magenta
            | ScalarTarget::Yellow
            | ScalarTarget::Black => doc.cmyk_plate_geometry(self.index),
            _ => {
                let layer = doc
                    .layer(self.index)
                    .ok_or("native scalar layer is out of range")?;
                let source = layer
                    .source()
                    .ok_or("native color plane requires a raster layer")?;
                let float_plane = source.storage() == PrecisionStorage::Rgba32Float
                    && self.float_filter(doc).is_some();
                if source.storage() != PrecisionStorage::Rgba16Unorm && !float_plane {
                    return Err("native scalar color plane requires u16 storage".into());
                }
                Ok((source.dimensions(), layer.offset()))
            }
        }
    }
    pub fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        let (dimensions, offset) = self.geometry(doc)?;
        let (w, h) = dimensions;
        let n = pixel_count(w, h)? as u64;
        self.admit_mask_growth(doc, dimensions)?;
        let (cw, ch) = doc.dimensions();
        let canvas = u64::from(cw) * u64::from(ch);
        let payload = match self.operation {
            ScalarOperation::Paint { .. }
            | ScalarOperation::DodgeBurn { .. }
            | ScalarOperation::ReplaceCanvas {} => canvas,
            ScalarOperation::Replace {} => n,
            _ => 0,
        };
        let input = payload + if self.selection { canvas } else { 0 };
        let float = self.float_filter(doc);
        // A float Red/Green/Blue/Alpha plane holds no u16 plane: its own
        // stages replace the `n·32` term (`float_plane_filter::plan`).
        let stored_f32 = float.is_some() && self.color_target();
        let planes = if stored_f32 { 0 } else { n * 32 };
        let mut scratch = 8 * BOOKKEEPING_BYTES + planes + input * 2;
        if let Some(edit) = float {
            scratch += crate::float_plane_filter::plan(
                edit,
                dimensions,
                self.float_plane(),
                stored_f32,
                doc,
            )?;
        } else if let ScalarOperation::Filter { edit } = &self.operation {
            scratch += crate::precision_scalar_filter::plan(edit, dimensions, doc.profile())?;
        }
        if let ScalarOperation::Morphology { operation } = &self.operation {
            scratch += operation.plan(dimensions)?;
        }
        if self.target.plate().is_some() {
            scratch += n * 2 * crate::cmyk_image::CMYK_BYTES_PER_PIXEL
                + canvas * crate::cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL;
        }
        Ok(
            json!({"retained_bytes":doc.retained_bytes(),"scratch_bytes":scratch,"input_samples":input,"payload_samples":payload,"selection_samples":input-payload,"width":w,"height":h,"x":offset.0,"y":offset.1,"input_width":cw,"input_height":ch,"input_planes":if canvas>0{input/canvas}else{0}}),
        )
    }
    /// Refuses a mask Invert whose grown plane (`geometry`) the mask model
    /// or the native mask budget cannot hold. Nothing to check when the
    /// plane keeps its stored size.
    fn admit_mask_growth(
        &self,
        doc: &PrecisionDocument,
        dimensions: (u32, u32),
    ) -> Result<(), String> {
        if !self.mask_invert {
            return Ok(());
        }
        let Some(mask) = doc.layer(self.index).and_then(|l| l.mask()) else {
            return Ok(());
        };
        let stored = mask.plane().dimensions();
        if stored == dimensions {
            return Ok(());
        }
        if !mask.properties().valid(dimensions) {
            return Err("native mask invert exceeds the mask's padded extent".into());
        }
        // The budget is a total, so any entry of the stored size stands for
        // this mask.
        let (rasters, mut masks) = doc.native_dimensions();
        if let Some(slot) = masks.iter_mut().find(|d| **d == stored) {
            *slot = dimensions;
        }
        doc.native_fits_geometry(doc.dimensions(), &rasters, &masks, doc.channels.len())
    }
    fn read(&self, doc: &PrecisionDocument, job: JobContext<'_>) -> Result<Vec<u16>, String> {
        match self.target {
            ScalarTarget::Channel => copy(doc.channels()[self.index].plane().samples(), job),
            ScalarTarget::Mask => {
                let mask = doc.layer(self.index).unwrap().mask().unwrap();
                let stored = (mask.plane().dimensions(), mask.properties().offset);
                let to = self.geometry(doc)?;
                if to == stored {
                    return copy(mask.plane().samples(), job);
                }
                // The grown plane of a selection-scoped Invert: the stored
                // samples, the outside constant everywhere else.
                job.check()?;
                let outside = u16::from(mask.properties().outside_coverage) * 257;
                crate::mask_invert::grown(mask.plane().samples(), stored, to, outside)
                    .ok_or_else(|| "could not allocate precision output".to_string())
            }
            ScalarTarget::Cyan
            | ScalarTarget::Magenta
            | ScalarTarget::Yellow
            | ScalarTarget::Black => {
                doc.cmyk_plate_samples(self.index, self.target.plate().unwrap_or(0), job)
            }
            _ => {
                // TILE-BAND order (each 256-row band, each 256-column span,
                // each row's run) into the pre-sized plane: a row across
                // every tile column costs ~20% at 24 MP (13B-3c-2a).
                // `job.every` reads only the visit counter `i`, so the
                // checkpoints are the flat walk's.
                let image = doc.layer_source(self.index).unwrap();
                if image.storage() != PrecisionStorage::Rgba16Unorm {
                    return Err("native scalar color plane requires u16 storage".into());
                }
                let (w, h) = image.dimensions();
                let mut out = reserved_vec(w as usize * h as usize)?;
                out.resize(w as usize * h as usize, 0);
                let mut i = 0;
                for band in (0..h).step_by(TILE_EDGE as usize) {
                    for x0 in (0..w).step_by(TILE_EDGE as usize) {
                        let x1 = x0.saturating_add(TILE_EDGE).min(w);
                        for y in band..band.saturating_add(TILE_EDGE).min(h) {
                            for (x, run) in image.row_segments(y, x0, x1) {
                                let PrecisionRun::Rgba16(run) = run else {
                                    return Err(
                                        "native scalar color plane requires u16 storage".into()
                                    );
                                };
                                let row = y as usize * w as usize + x as usize;
                                for (k, p) in run.iter().enumerate() {
                                    job.every(i)?;
                                    out[row + k] = p[self.component()];
                                    i += 1;
                                }
                            }
                        }
                    }
                }
                Ok(out)
            }
        }
    }
    /// The filter of a float plane edit: Some for a filter
    /// (not an adjustment, not Auto) in a 32-bit float RGB document.
    fn float_filter<'a>(&'a self, doc: &PrecisionDocument) -> Option<&'a SelectedEdit> {
        let ScalarOperation::Filter { edit } = &self.operation else {
            return None;
        };
        (doc.is_float() && !doc.is_gray() && edit.op != "adjustment" && !edit.is_auto())
            .then_some(edit)
    }
    /// Whether the target is one component of a layer's RGBA.
    fn color_target(&self) -> bool {
        matches!(
            self.target,
            ScalarTarget::Red | ScalarTarget::Green | ScalarTarget::Blue | ScalarTarget::Alpha
        )
    }
    /// The plane rule's side of this target (`float_plane_filter`).
    fn float_plane(&self) -> crate::float_plane_filter::FloatPlane {
        match self.target {
            ScalarTarget::Red | ScalarTarget::Green | ScalarTarget::Blue => {
                crate::float_plane_filter::FloatPlane::Color
            }
            _ => crate::float_plane_filter::FloatPlane::Coverage,
        }
    }
    fn component(&self) -> usize {
        match self.target {
            ScalarTarget::Red => 0,
            ScalarTarget::Green => 1,
            ScalarTarget::Blue => 2,
            ScalarTarget::Gray => 0,
            _ => 3,
        }
    }
    fn local_coverage(
        &self,
        doc: &PrecisionDocument,
        coverage: &[u16],
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        if !matches!(&self.operation,ScalarOperation::Filter{edit} if edit.analysis_scope==AnalysisScope::Selection)
            || coverage.is_empty()
        {
            return Ok(Vec::new());
        }
        let (size, offset) = self.geometry(doc)?;
        let count = pixel_count(size.0, size.1)?;
        let mut local = reserved_vec(count)?;
        for i in 0..count {
            job.every(i)?;
            local.push(coverage_at(coverage, doc.dimensions(), size, offset, i));
        }
        Ok(local)
    }
    pub(crate) fn auto_levels(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Value, String> {
        let ScalarOperation::Filter { edit } = &self.operation else {
            return Err("scalar Auto needs a filter request".into());
        };
        if !edit.is_auto() {
            return Err("scalar Auto derivation requires an automatic filter".into());
        }
        let plan = self.plan(doc)?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap();
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        if input.len() != plan["input_samples"].as_u64().unwrap() as usize {
            return Err("scalar Auto coverage count differs from plan".into());
        }

        let samples = self.read(doc, job)?;
        let local = self.local_coverage(doc, input, job)?;
        let output = crate::precision_scalar_filter::auto_levels(
            edit,
            &samples,
            self.geometry(doc)?.0,
            doc.profile(),
            &local,
            job,
        )?;
        job.report(1., 5)?;
        Ok(output)
    }
    /// The LOCK gate (`precision_lock`). Which kind a scalar edit is depends
    /// on its TARGET, exactly as at byte depth: an RGBA plane is a `Pixels`
    /// edit, a layer MASK is a `Mask` edit (Lock Pixels deliberately does not
    /// block painting a mask — `doc_lock`'s module doc), and a document
    /// CHANNEL belongs to no layer at all, so no lock addresses it.
    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let kind = match self.target {
            ScalarTarget::Channel => None,
            ScalarTarget::Mask => Some(crate::doc_lock::EditKind::Mask),
            ScalarTarget::Red
            | ScalarTarget::Green
            | ScalarTarget::Blue
            | ScalarTarget::Alpha
            | ScalarTarget::Cyan
            | ScalarTarget::Magenta
            | ScalarTarget::Yellow
            | ScalarTarget::Black
            | ScalarTarget::Gray => Some(crate::doc_lock::EditKind::Pixels),
        };
        let Some(kind) = kind else {
            return self.applying_unlocked(doc, input, limit, other, job);
        };
        doc.under_locks(self.index, kind, |doc| {
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
        let scratch = plan["scratch_bytes"].as_u64().unwrap();
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        if input.len() != plan["input_samples"].as_u64().unwrap() as usize {
            return Err("native scalar input count differs from plan".into());
        }
        if matches!(self.operation, ScalarOperation::DodgeBurn { exposure, .. } if exposure == 0.) {
            job.report(1., 5)?;
            return Ok(None);
        }
        let (dimensions, offset) = self.geometry(doc)?;
        let (w, h) = dimensions;
        let payload = plan["payload_samples"].as_u64().unwrap() as usize;
        let selection = &input[payload..];
        let input = &input[..payload];
        let float = self.float_filter(doc);
        if let (Some(edit), true) = (float, self.color_target()) {
            return doc.float_plane_filter_unlocked(
                self.index,
                self.component(),
                edit,
                selection,
                job,
            );
        }
        let before = self.read(doc, job)?;
        let region = if let ScalarOperation::BucketFill {
            x,
            y,
            tolerance,
            contiguous,
            ..
        } = &self.operation
        {
            let sx = i64::from(*x) - i64::from(offset.0);
            let sy = i64::from(*y) - i64::from(offset.1);
            if sx < 0 || sy < 0 || sx >= i64::from(w) || sy >= i64::from(h) {
                job.report(1., 5)?;
                return Ok(None);
            }
            Some(crate::selection_region::grow(
                dimensions,
                |i| [before[i], before[i], before[i], 65535],
                (sx as u32, sy as u32),
                *tolerance,
                *contiguous,
                1,
                job.range(0., 0.6),
            )?)
        } else {
            None
        };
        let transformed = if let ScalarOperation::Morphology { operation } = &self.operation {
            Some(operation.apply(&before, dimensions, job.range(0., 0.7))?)
        } else {
            None
        };
        let local = self.local_coverage(doc, selection, job)?;
        let filtered = if let ScalarOperation::Filter { edit } = &self.operation {
            // Radial Blur and Offset act within the selection's bounding box
            // on this plane, derived now from the same coverage the write
            // reads (`with_frame` ignores it for every other filter).
            let framed = matches!(edit.op.as_str(), "radial_blur" | "offset");
            let frame = if selection.is_empty() || !framed {
                None
            } else {
                crate::ops_spatial::coverage_frame(dimensions, |i| {
                    coverage_at(selection, doc.dimensions(), dimensions, offset, i)
                })
            };
            if float.is_some() {
                let codes = crate::float_plane_filter::coverage_codes(
                    edit,
                    &before,
                    dimensions,
                    frame,
                    doc.profile(),
                    job.range(0., 0.7),
                )?;
                if codes.is_none() {
                    job.report(1., 5)?;
                    return Ok(None);
                }
                codes
            } else {
                Some(crate::precision_scalar_filter::evaluate(
                    edit,
                    &before,
                    dimensions,
                    doc.profile(),
                    &local,
                    frame,
                    job.range(0., 0.7),
                )?)
            }
        } else {
            None
        };
        // A non-Normal Fill/Paint blends the value over the plane's gray
        // through `doc_plane::blend_triple` (Dissolve at canvas coordinates);
        // Normal keeps the lerp below.
        let blend = match self.operation {
            ScalarOperation::Fill {
                blend_mode,
                opacity,
                ..
            }
            | ScalarOperation::Paint {
                blend_mode,
                opacity,
                ..
            } => {
                let mode = crate::precision_plane_math::validate(1, blend_mode, opacity)?;
                (mode != crate::blend::BlendMode::Normal).then(|| crate::blend::blend_kind(mode))
            }
            _ => None,
        };
        let mut out = reserved_vec(before.len())?;
        let mut changed = false;
        for (i, &original) in before.iter().enumerate() {
            job.every(i)?;
            if i & 65535 == 0 {
                job.report(0.7 + 0.3 * i as f64 / before.len() as f64, 6)?;
            }
            if matches!(
                self.operation,
                ScalarOperation::ReplaceCanvas {} | ScalarOperation::Gradient { .. }
            ) {
                let x = (i % w as usize) as i64 + i64::from(offset.0);
                let y = (i / w as usize) as i64 + i64::from(offset.1);
                if x < 0 || y < 0 || x >= i64::from(doc.width) || y >= i64::from(doc.height) {
                    out.push(original);
                    continue;
                }
            }
            let weight = coverage_at(selection, doc.dimensions(), dimensions, offset, i);
            if weight == 0 {
                out.push(original);
                continue;
            }
            let (value, opacity, stroke) = match &self.operation {
                ScalarOperation::Fill { value, opacity, .. } => {
                    (f64::from(*value), *opacity, 65535)
                }
                ScalarOperation::Paint { value, opacity, .. } => (
                    f64::from(*value),
                    *opacity,
                    coverage_at(input, doc.dimensions(), dimensions, offset, i),
                ),
                ScalarOperation::Gradient {
                    from,
                    to,
                    radial,
                    start,
                    end,
                    start_alpha,
                    end_alpha,
                    opacity,
                    stops,
                    style,
                    smoothness,
                } => {
                    let point = [
                        (i % w as usize) as f64 + f64::from(offset.0) + 0.5,
                        (i / w as usize) as f64 + f64::from(offset.1) + 0.5,
                    ];
                    if stops.is_empty() {
                        let t = crate::gradient_field::parameter(point, *from, *to, *radial);
                        // The drag's gray is the Classic ramp where its
                        // style is identified (`FieldStyle::classic`); alpha stays linear.
                        let drag = if *radial {
                            FieldStyle::Radial
                        } else {
                            FieldStyle::Linear
                        };
                        let gray = if !drag.classic() {
                            f64::from(*start) * (1. - t) + f64::from(*end) * t
                        } else {
                            let [g] = crate::gradient_field::classic_two(
                                [*start as f32],
                                [*end as f32],
                                t as f32,
                            );
                            f64::from(g)
                        };
                        (
                            gray,
                            *opacity * (*start_alpha * (1. - t) + *end_alpha * t),
                            65535,
                        )
                    } else {
                        // `gradient_field::scalar_over`'s terms, left to the
                        // one lerp below so selection coverage scales them.
                        let style = style.unwrap_or(if *radial {
                            FieldStyle::Radial
                        } else {
                            FieldStyle::Linear
                        });
                        let t = crate::gradient_field::styled_parameter(point, *from, *to, style);
                        let color =
                            crate::gradient_field::stop_color(stops, t as f32, style, *smoothness);
                        (
                            f64::from(color[0]) * 65535.,
                            *opacity * f64::from(color[3]),
                            65535,
                        )
                    }
                }
                ScalarOperation::DodgeBurn {
                    exposure,
                    range,
                    burn,
                    ..
                } => (
                    f64::from(crate::doc_retouch::shaped(
                        f32::from(original) / 65535.,
                        *exposure,
                        *range,
                        *burn,
                    )) * 65535.,
                    1.,
                    coverage_at(input, doc.dimensions(), dimensions, offset, i),
                ),
                ScalarOperation::BucketFill { value, opacity, .. } => {
                    (f64::from(*value), *opacity, region.as_ref().unwrap()[i])
                }
                ScalarOperation::Replace {} => (f64::from(input[i]), 1., 65535),
                ScalarOperation::ReplaceCanvas {} => (
                    f64::from(coverage_at(input, doc.dimensions(), dimensions, offset, i)),
                    1.,
                    65535,
                ),
                ScalarOperation::Morphology { .. } => {
                    (f64::from(transformed.as_ref().unwrap()[i]), 1., 65535)
                }
                ScalarOperation::Filter { .. } => (filtered.as_ref().unwrap()[i], 1., 65535),
            };
            let t = (f64::from(weight) / 65535.) * (f64::from(stroke) / 65535.) * opacity;
            let value = if t == 0. {
                original
            } else if let Some(kind) = blend {
                let o = f32::from(original) / 65535.;
                let v = (value / 65535.) as f32;
                let xy = (
                    (i % w as usize) as i64 + i64::from(offset.0),
                    (i / w as usize) as i64 + i64::from(offset.1),
                );
                let result = crate::doc_plane::blend_triple([o; 3], [v; 3], t as f32, kind, xy)[0];
                (result.clamp(0., 1.) * 65535.).round() as u16
            } else {
                (f64::from(original) * (1. - t) + value * t)
                    .clamp(0., 65535.)
                    .round() as u16
            };
            changed |= value != original;
            out.push(value);
        }
        job.check()?;
        if !changed {
            job.report(1., 5)?;
            return Ok(None);
        }
        let result = match self.target {
            ScalarTarget::Mask => {
                let mut layer = doc.layer(self.index).unwrap().clone();
                let mask = layer.mask.as_mut().unwrap();
                mask.plane = PrecisionPlane::adopt(w, h, out)?;
                if self.mask_invert {
                    if self.selection {
                        // The plane may have grown to hold the canvas.
                        mask.properties.offset = offset;
                    } else {
                        // Everything inverts, the outside constant with it.
                        mask.properties.outside_coverage = 255 - mask.properties.outside_coverage;
                    }
                }
                doc.replacing_layer(self.index, layer)?
            }
            ScalarTarget::Channel => {
                let mut channels = doc.channels.as_ref().clone();
                channels[self.index].plane = PrecisionPlane::adopt(w, h, out)?;
                Some(doc.rebuilt(doc.width, doc.height, doc.layers.as_ref().clone(), channels)?)
            }
            ScalarTarget::Cyan
            | ScalarTarget::Magenta
            | ScalarTarget::Yellow
            | ScalarTarget::Black => {
                doc.replacing_cmyk_plate(self.index, self.target.plate().unwrap_or(0), &out, job)?
            }
            _ => {
                let layer_image = doc.layer_source(self.index).unwrap();
                let runs = layer_image
                    .runs_rgba16()
                    .ok_or("native scalar color plane requires u16 storage")?;
                // One `extend_from_slice` per run in row order, then ONE
                // flat pass with HEAD's index sequence and statement.
                let mut pixels = reserved_vec(w as usize * h as usize)?;
                for run in runs {
                    pixels.extend_from_slice(run);
                }
                let gray = matches!(self.target, ScalarTarget::Gray);
                for (i, p) in pixels.iter_mut().enumerate() {
                    job.every(i)?;
                    if gray {
                        p[..3].fill(out[i]);
                    } else {
                        p[self.component()] = out[i];
                    }
                }
                let image = layer_image.rebuilt_rgba16(w, h, pixels, Arc::clone(doc.profile()))?;
                doc.replacing_content(self.index, Content::Raster { image, offset })?
            }
        };
        job.report(1., 5)?;
        Ok(result)
    }
}
