//! Native retouch commands over captured sampling plates. Source interpolation,
//! destination storage growth and output placement are explicit; every result
//! is immutable, capacity checked, cancellable, and exact outside write coverage.
//!
//! Since 13B-9c a request may name its `coverage_rect` (the stroke and the
//! selection arrive over it; `retouch_coverage`), the selection domain covers
//! the stroke's bbox only, and the in-place arm's plan charges what its rect
//! walk holds (`precision_retouch_plan`); the other arms plan the canvas.
//!
//! This module holds the request and its plan; the run (`retouch`,
//! `retouch_from`) is `precision_retouch_run`.
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{pixel_count, BOOKKEEPING_BYTES};
use crate::precision_paint::PaintPlan;
use crate::retouch_sampling::RetouchSampling;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RetouchOp {
    RetouchClone,
    RetouchHeal,
    SpotHeal,
    ContentAwareFill,
    RedEye,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RetouchOutput {
    #[default]
    Layer,
    NewLayer,
}
fn current() -> String {
    "current".into()
}
fn identity() -> [f64; 6] {
    [1., 0., 0., 1., 0., 0.]
}
fn one() -> f32 {
    1.
}
fn half() -> f32 {
    0.5
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetouchEdit {
    pub op: RetouchOp,
    pub layer: usize,
    #[serde(default)]
    pub source_layer: Option<usize>,
    #[serde(default = "current")]
    pub sample: String,
    #[serde(default)]
    pub ignore_adjustments: bool,
    #[serde(default)]
    pub output: RetouchOutput,
    #[serde(default)]
    pub expand_destination: bool,
    #[serde(default)]
    pub selection: bool,
    #[serde(default = "identity")]
    pub matrix: [f64; 6],
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub ring: u32,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub source_mask: Option<crate::inpaint_source_mask::SourceMask>,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default = "one")]
    pub pupil_size: f32,
    #[serde(default = "half")]
    pub darken: f32,
    /// `DrBlendMode` raw value the cloned paint composites through
    /// (`retouch_clone` only, in place; 0 = Normal).
    #[serde(default)]
    pub blend_mode: u32,
    /// The Spot Healing Brush's Type (`spot_heal` only; absent =
    /// Content-Aware, the meaning every earlier request had).
    #[serde(default, rename = "type")]
    pub spot_type: crate::spot_heal_type::SpotHealType,
    /// The hole's covered count (`content_aware_fill` only): the request's
    /// coverage bytes at or above `doc_inpaint::Caller::Fill`'s hole threshold
    /// (1, so a feathered ramp counts), which sets the admission's
    /// `inpaint_lift` before the coverage is read; the edit refuses a count
    /// that is not its coverage's own (`doc_content_aware`'s `# Admission`).
    #[serde(default)]
    pub covered: Option<u64>,
    /// Content-Aware Fill Transparent Areas' pad (`content_aware_fill` with
    /// `output: "layer"` only): the hole's transparent
    /// destination pixels are made opaque before the fill writes them
    /// (`precision_inpaint::opaque_hole`), so the fill shows there. Absent =
    /// false, every earlier request's meaning: a hole over transparent pixels
    /// stays transparent.
    #[serde(default)]
    pub fill_transparent: bool,
    /// The canvas rect `[x, y, w, h]` the request's coverage samples cover
    /// (13B-9c, `retouch_coverage`): the stroke, then with `selection` the
    /// selection, each row-major over the rect, and 0 outside it. Absent =
    /// the canvas, every earlier request's meaning. Red Eye refuses it.
    #[serde(default)]
    pub coverage_rect: Option<[u32; 4]>,
    /// The full-frame seam (tests only, never read from JSON): a Clone or
    /// Heal transforms the whole canvas from the whole plate, the path
    /// before 13B-9a's rect (`precision_retouch_apply`'s module doc), and a
    /// `sample: current` Spot Heal or Content-Aware Fill renders the canvas
    /// plate and plans its hole over the canvas, the path before 13B-9b's
    /// in-place read ([`RetouchEdit::reads_plate_in_place`]), as the oracles
    /// the rect and in-place paths are tested against.
    #[serde(skip)]
    #[doc(hidden)]
    pub full_frame: bool,
}
const FILL_TRANSPARENT_ONLY: &str =
    "fill_transparent applies only to content_aware_fill with output layer";
impl RetouchEdit {
    pub(crate) fn output_name(&self) -> &'static str {
        if self.op == RetouchOp::ContentAwareFill {
            "Content-Aware Fill"
        } else {
            "Retouch"
        }
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 64 * 1024 {
            return Err("retouch request exceeds 64 KiB".into());
        }
        let edit: Self = serde_json::from_str(text)
            .map_err(|e| format!("invalid native retouch request: {e}"))?;
        let fields: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        // The transparent pad is the in-place fill's alone, named before any
        // other key so the refusal does not depend on the keys' order.
        if fields.get("fill_transparent").is_some() && edit.op != RetouchOp::ContentAwareFill {
            return Err(FILL_TRANSPARENT_ONLY.into());
        }
        for key in fields
            .as_object()
            .ok_or("retouch request must be an object")?
            .keys()
        {
            let allowed = match edit.op {
                RetouchOp::RedEye => [
                    "op",
                    "layer",
                    "x",
                    "y",
                    "width",
                    "height",
                    "pupil_size",
                    "darken",
                ]
                .contains(&key.as_str()),
                RetouchOp::ContentAwareFill => [
                    "op",
                    "layer",
                    "ring",
                    "seed",
                    "sample",
                    "ignore_adjustments",
                    "output",
                    "source_mask",
                    "x",
                    "y",
                    "width",
                    "height",
                    "covered",
                    "fill_transparent",
                    "coverage_rect",
                ]
                .contains(&key.as_str()),
                RetouchOp::SpotHeal => {
                    !["matrix", "pupil_size", "darken", "source_mask", "covered"]
                        .contains(&key.as_str())
                }
                _ => ![
                    "ring",
                    "seed",
                    "pupil_size",
                    "darken",
                    "source_mask",
                    "covered",
                ]
                .contains(&key.as_str()),
            };
            // A blend mode composites a SOURCE colour; only the clone has one.
            let allowed = allowed && (key != "blend_mode" || edit.op == RetouchOp::RetouchClone);
            // The Type is the Spot Healing Brush's alone.
            let allowed = allowed && (key != "type" || edit.op == RetouchOp::SpotHeal);
            if !allowed {
                return Err(format!(
                    "{key} is not an argument of this retouch operation"
                ));
            }
        }
        if edit.fill_transparent && edit.output != RetouchOutput::Layer {
            return Err(FILL_TRANSPARENT_ONLY.into());
        }
        Ok(edit)
    }
    pub(crate) fn mode(&self) -> Result<RetouchSampling, String> {
        match self.sample.as_str() {
            "current" => Ok(RetouchSampling::Current),
            "current_and_below" => Ok(RetouchSampling::CurrentAndBelow),
            "all" => Ok(RetouchSampling::All),
            _ => Err("sample must be current, current_and_below or all".into()),
        }
    }
    pub(crate) fn extent(
        &self,
        doc: &PrecisionDocument,
    ) -> Result<crate::retouch_transform::StorageExtent, String> {
        let target = doc
            .layer(self.layer)
            .ok_or("retouch target layer is out of range")?;
        let image = target.source().ok_or("retouch requires a raster target")?;
        if self.output == RetouchOutput::NewLayer {
            return Ok((doc.dimensions(), (0, 0)));
        }
        if self.expand_destination {
            crate::retouch_transform::expanded_extent(
                doc.dimensions(),
                image.dimensions(),
                target.offset(),
            )
        } else {
            Ok((image.dimensions(), target.offset()))
        }
    }
    pub(crate) fn bounds(&self, doc: &PrecisionDocument) -> Result<(u32, u32, u32, u32), String> {
        if self.x < 0
            || self.y < 0
            || self.width == 0
            || self.height == 0
            || u64::from(self.x as u32) + u64::from(self.width) > u64::from(doc.width)
            || u64::from(self.y as u32) + u64::from(self.height) > u64::from(doc.height)
        {
            return Err("retouch bounds must be a nonempty rectangle inside the canvas".into());
        }
        Ok((self.x as u32, self.y as u32, self.width, self.height))
    }
    /// The canvas rect `(x, y, w, h)` the coverage samples cover: the
    /// request's `coverage_rect`, or the canvas when it is absent.
    pub(crate) fn coverage_area(
        &self,
        doc: &PrecisionDocument,
    ) -> Result<(u32, u32, u32, u32), String> {
        let Some([x, y, w, h]) = self.coverage_rect else {
            return Ok((0, 0, doc.width, doc.height));
        };
        if w == 0
            || h == 0
            || u64::from(x) + u64::from(w) > u64::from(doc.width)
            || u64::from(y) + u64::from(h) > u64::from(doc.height)
        {
            return Err(
                "retouch coverage_rect must be a nonempty rectangle inside the canvas".into(),
            );
        }
        Ok((x, y, w, h))
    }
    /// SpotHeal|ContentAwareFill, mode() == Ok(Current), !full_frame.
    pub(crate) fn reads_plate_in_place(&self) -> bool {
        matches!(self.op, RetouchOp::SpotHeal | RetouchOp::ContentAwareFill)
            && matches!(self.mode(), Ok(RetouchSampling::Current))
            && !self.full_frame
    }
    /// self.bounds(doc), or (0, 0, w, h) under full_frame.
    pub(crate) fn synthesis_domain(
        &self,
        doc: &PrecisionDocument,
    ) -> Result<(u32, u32, u32, u32), String> {
        if self.full_frame {
            return Ok((0, 0, doc.width, doc.height));
        }
        self.bounds(doc)
    }
}
impl PrecisionDocument {
    pub fn retouch_plan(&self, edit: &RetouchEdit) -> Result<PaintPlan, String> {
        if self.is_cmyk() {
            return self.cmyk_retouch_plan(edit);
        }
        // A 32-bit float document's own refusal first: a clone mode
        // outside the float blend set.
        let float = self.is_float();
        if float {
            crate::float_retouch::plan_refusal(edit)?;
        }
        let mode = edit.mode()?;
        if !edit.opacity.is_finite() || !(0.0..=1.0).contains(&edit.opacity) || edit.ring > 512 {
            return Err("retouch opacity must be normalized; ring must be 0 through 512".into());
        }
        if edit.source_layer.is_some() && mode != RetouchSampling::Current {
            return Err("source_layer requires sample=current".into());
        }
        // The blend enum's own answer, as the Clone arm and `gradient_stops`
        // ask it: an unknown value, or the group-only Pass Through.
        match crate::blend::BlendMode::from_c(edit.blend_mode as i32) {
            Some(mode) if !mode.is_group_only() => {}
            _ => return Err("retouch blend mode is invalid".into()),
        }
        if edit.blend_mode != 0 && edit.output == RetouchOutput::NewLayer {
            return Err(
                "a clone onto a new layer has nothing to blend with; blend_mode requires output layer"
                    .into(),
            );
        }
        crate::retouch_transform::validate(&edit.matrix)?;
        let (_, _, aw, ah) = edit.coverage_area(self)?;
        let area = u64::from(aw) * u64::from(ah);
        let target = self
            .layer(edit.layer)
            .ok_or("retouch target layer is out of range")?;
        if target.retained_source.is_some() && edit.output == RetouchOutput::Layer {
            return Err("Rasterize the retained source before retouching its pixels".into());
        }
        let (dimensions, _) = edit.extent(self)?;
        let n = pixel_count(self.width, self.height)? as u64;
        let output = pixel_count(dimensions.0, dimensions.1)? as u64;
        // The in-place arm plans what its rect walk holds
        // (`precision_retouch_plan`, 13B-9c); every other arm the canvas.
        let in_place = edit.output == RetouchOutput::Layer
            && !edit.expand_destination
            && !edit.full_frame
            && edit.op != RetouchOp::RedEye;
        let buffers = n * (if edit.selection { 44 } else { 40 }) + output * 16;
        let mut scratch = 8 * BOOKKEEPING_BYTES;
        if !in_place {
            scratch += buffers;
        }
        if float && !in_place {
            // The pixel buffers at f32 width (`float_retouch`'s module doc
            // holds the measurement), and for the ops that solve in W codes
            // (`retouch_encoding`) the encoded copy of the destination.
            scratch += buffers;
            if matches!(
                edit.op,
                RetouchOp::RetouchHeal | RetouchOp::SpotHeal | RetouchOp::ContentAwareFill
            ) {
                scratch += output * 16;
            }
        }
        let mut plate = 0;
        if edit.op != RetouchOp::RedEye {
            // Validated always; charged unless the synthesis reads the source
            // layer in place, with no plate (13B-9b), or (in place) where
            // the walk reads it.
            plate = self.retouch_source_plan(edit.source_layer.unwrap_or(edit.layer), mode)?;
            if !in_place && !edit.reads_plate_in_place() {
                scratch += plate;
            }
        }
        if edit.output == RetouchOutput::NewLayer {
            self.retouch_output_refusal(edit.layer)?;
            self.preflight_raster_addition(self.dimensions(), edit.output_name())?;
            scratch += self.retouch_source_plan(edit.layer, RetouchSampling::CurrentAndBelow)?;
        } else {
            let (mut r, m) = self.native_dimensions();
            let raster_index = (0..edit.layer)
                .filter(|i| self.layer_source(*i).is_some())
                .count();
            r[raster_index] = dimensions;
            self.native_fits_geometry(self.dimensions(), &r, &m, self.channels.len())?;
        }
        if in_place {
            scratch += self.in_place_retouch_scratch(edit, mode, plate, float)?;
        } else if edit.op == RetouchOp::RetouchHeal {
            let area = if edit.width > 0 && edit.height > 0 {
                let (_, _, w, h) = edit.bounds(self)?;
                u64::from(w) * u64::from(h)
            } else {
                n
            };
            let padded = if edit.width > 0 && edit.height > 0 {
                u64::from(edit.width.saturating_add(2).min(self.width))
                    * u64::from(edit.height.saturating_add(2).min(self.height))
            } else {
                area
            };
            scratch += crate::precision_heal::plan(padded)?.0;
        } else if matches!(edit.op, RetouchOp::SpotHeal | RetouchOp::ContentAwareFill) {
            let bounds = edit.bounds(self)?;
            let caller = if edit.op == RetouchOp::ContentAwareFill {
                crate::doc_inpaint::Caller::Fill
            } else {
                crate::doc_inpaint::Caller::SpotHeal
            };
            scratch += crate::precision_inpaint::estimate(
                self.dimensions(),
                bounds,
                u64::from(bounds.2) * u64::from(bounds.3),
                edit.ring,
                caller,
                edit.spot_type,
                3,
                n,
            )?
            .0;
        }
        if let Some(mask) = &edit.source_mask {
            mask.validate(self.width, self.height)?;
        }
        if edit.op == RetouchOp::RedEye {
            if edit.width == 0 || edit.height == 0 {
                return Err("red eye rectangle must be nonempty".into());
            }
            if !edit.pupil_size.is_finite()
                || !(0.01..=1.).contains(&edit.pupil_size)
                || !edit.darken.is_finite()
                || !(0.0..=1.).contains(&edit.darken)
            {
                return Err(
                    "red eye pupil_size and darken are outside their normalized ranges".into(),
                );
            }
            if edit.output != RetouchOutput::Layer
                || edit.expand_destination
                || edit.source_layer.is_some()
                || mode != RetouchSampling::Current
                || edit.ignore_adjustments
            {
                return Err("red eye corrects its current raster target in place".into());
            }
            // A Lab layer's ProPhoto work copy and pipelines (#1 Lab 2f).
            if self.is_lab() {
                scratch =
                    scratch.saturating_add(crate::lab_retouch::red_eye_scratch_bytes(output)?);
            }
        }
        Ok(PaintPlan {
            retained_bytes: self.retained_bytes(),
            scratch_bytes: scratch,
            input_samples: area as usize * if edit.selection { 2 } else { 1 },
        })
    }
}
