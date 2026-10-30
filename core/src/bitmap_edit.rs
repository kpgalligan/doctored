//! The edits a Bitmap document takes (#547 Bitmap phases 2a-i and 2a-ii,
//! `designs/bitmap-mode.md`): painting in black and white, the
//! geometry, the selected pixels' moves and transforms, and Cut. An
//! adapter, not a kernel: each request runs on the document's 8-bit Gray
//! storage (`PrecisionDocument::without_bitmap`) through its existing
//! kernel, then [`republish`] puts every pixel back on the 1-bit plane by
//! the one rule, `bitmap_method::white_over`.
//!
//! The ops (`bitmap_document::admit_edit` routes them here): the Brush and Fill
//! (`paint_blend`), the Paint Bucket (`bucket_fill`) and Stroke… with
//! nothing selected (`stroke_layer_edge`), in Normal mode; the scalar paint
//! (`paint`); the Eraser (`erase_native`) and Clear (`erase`); Crop, Canvas
//! Size, the quarter turns, the flips and `straighten` (the Crop tool's
//! Delete Cropped, past-canvas crop and straighten); since phase 2a-ii
//! Image Size (`resize`), the layer ops Move uses (`affine`, `perspective`,
//! `affine_set`, `perspective_set`), the selected pixels' `move_selected`
//! (the layer target only) and Edit ▸ Cut ([`cut`]). Since phase 2b
//! arbitrary rotation (`rotate_canvas`) and `transform_selected` refuse for
//! good at the gate. Since phase 3 the Gradient (`gradient`: the Gray
//! storage's ramp thresholded, with no ink snap, so it lands where the ramp
//! crosses 128), Perspective Crop (`perspective_crop`: outside the quad
//! takes the background bit) and the Clone Stamp (`retouch_clone` in place:
//! `indexed_paint`'s Clone arm; a clone onto a new layer refuses as
//! `add_layer`). Liquify and the History Brush are `bitmap_tools`'.
//! A resampled op runs with its own sampler on the gray and is then
//! thresholded; area it uncovers or vacates comes back alpha 0 and takes
//! the background bit; a whole-pixel move or a nearest sampler keeps each
//! bit.
//!
//! - **The ink snap.** A paint's `color` is prepared on the Gray storage as
//!   the paint itself prepares it (`precision_paint::prepare_colors`, the
//!   Gray luminance), its 8-bit code snapped by `white_over(code, 255, _)`
//!   to black or white, and the inner op re-parsed with that ink in
//!   `document` space. The opacity (and the colour's alpha) is kept: a
//!   stroke under 50 % moves no pixel past the threshold, so it changes
//!   nothing.
//! - **The rule.** Each canvas pixel of the result's one layer (read
//!   through its offset by `indexed_paint::canvas_row`, the one window
//!   reader, so a stroke that grew the layer is clipped and the new area of
//!   a crop or Canvas Size is alpha 0) is white iff its gray over the
//!   background at its alpha reaches 50% Threshold's 128. A soft edge is
//!   thresholded, not turned off: it lands where its coverage reaches about
//!   half.
//! - **The background** is a Straighten's own `background` first (an MCP
//!   past-canvas `crop` with Delete Cropped off pads nothing on the Gray
//!   storage, so its new area comes back clear for this colour to fill),
//!   else the request's `indexed_paint::BACKGROUND_KEY` (the host's
//!   background swatch), else white (`indexed_geometry`'s order), each read
//!   on the Gray storage by `indexed_paint::background_codes` and snapped by
//!   the same rule at full alpha.
//! - **Purity.** A plane, canvas, resolution and chrome (guides, ruler
//!   origin, colour samplers, paths and comps) equal to the old ones are
//!   `None`; so a quarter turn of a symmetric plane still swaps an
//!   anisotropic resolution, and a flip of a uniform plane still moves its
//!   guides. Otherwise the one "Bitmap" layer is re-rendered from the new
//!   policy at the origin with no warp and the document rebuilt
//!   (`indexed_paint::rebuild`'s shape), so the `.rz` writer's
//!   `bitmap_native::check` accepts it.
//!
//! The plan is the inner op's plan on the Gray storage plus the output
//! canvas × [`BITMAP_EDIT_BYTES_PER_PIXEL`] and bookkeeping (Cut: the
//! canvas, beside the payload held twice).

use std::sync::Arc;

use serde_json::{json, Value};

use crate::bitmap_document::{refused_sentence, BitmapPolicy};
use crate::bitmap_method::{pack_row, stride, white_over};
use crate::indexed_paint::{background_codes, canvas_row, split_background};
use crate::indexed_palette::code8;
use crate::job_control::{DrJobControl, JobContext};
use crate::perspective_crop::PerspectiveCropRequest;
use crate::precision_clipboard::ClipboardPayload;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_fill_stroke::LayerEdgeStroke;
use crate::precision_geometry::{CropBackground, GeometryEdit};
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::precision_paint::{PaintColorSpace, PaintEdit};
use crate::precision_paint_tools::NativePaintEdit;
use crate::precision_retouch::RetouchEdit;
use crate::selected_pixels::{SelectedPixelsEdit, SelectedTarget};

/// The adapter's working memory per output pixel, beside the inner op's
/// own: the new plane, the re-rendered RGBA16 layer, one row of grays and
/// the host's display preparation of the result. Declared before measuring
/// from `to-gray-from-bitmap`'s measured cost of the same rebuild
/// (`scripts/bitmap-admission/measure.py`): max E 576,438,680 over 24 MP is
/// 24.02 B/px, × 1.25 is 30.02, so 31 was declared. At 31
/// `crop-extend-bitmap` (a 35 MP output whose inner plan is near zero,
/// Release, 3 runs) missed: plan 1,091,294,406 against the 1.25× need of
/// 1,168,036,650 (max E 934,429,320). The need is 33.2 B/px beside the inner
/// plan and bookkeeping, so 34. A miss raises this term only.
pub(crate) const BITMAP_EDIT_BYTES_PER_PIXEL: u64 = 34;

/// The refusal on any document but a Bitmap one.
pub(crate) const NOT_BITMAP: &str = "the document is not a Bitmap document";

/// One edit on a Bitmap document (module doc).
pub(crate) struct BitmapEdit {
    /// The request with `indexed_paint::BACKGROUND_KEY` taken off, which the
    /// ink snap re-parses.
    request: Value,
    op: BitmapOp,
    /// The request's `indexed_paint::BACKGROUND_KEY` (module doc).
    background: Option<CropBackground>,
}

/// The existing kernel a [`BitmapEdit`] runs.
enum BitmapOp {
    Native(NativePaintEdit),
    Legacy(PaintEdit),
    Edge(LayerEdgeStroke),
    Geometry(GeometryEdit),
    Selected(SelectedPixelsEdit),
    /// Perspective Crop (phase 3): outside the quad takes the background bit.
    PerspectiveCrop(PerspectiveCropRequest),
    /// The Clone Stamp (`retouch_clone` only, in place, phase 3):
    /// `indexed_paint`'s Clone arm on the Gray storage.
    Clone(RetouchEdit),
}

impl BitmapOp {
    /// The kernel of `text`'s `op`; every op the gate does not admit
    /// refuses with the mode's sentence, and so does a selected pixels'
    /// edit of anything but the layer (a Bitmap document holds no mask, no
    /// alpha channel and no colour plane to float) and a clone onto a new
    /// layer (as `add_layer` does).
    fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("invalid high-depth edit request: {e}"))?;
        Ok(match value["op"].as_str().unwrap_or("edit") {
            "paint_blend" | "bucket_fill" | "erase_native" | "gradient" => {
                Self::Native(NativePaintEdit::parse(text)?)
            }
            "perspective_crop" => Self::PerspectiveCrop(PerspectiveCropRequest::parse(text)?),
            "retouch_clone" => {
                if value["output"].as_str() == Some("new_layer") {
                    return Err(refused_sentence("add_layer"));
                }
                Self::Clone(RetouchEdit::parse(text)?)
            }
            "paint" | "erase" => Self::Legacy(PaintEdit::parse(text)?),
            "stroke_layer_edge" => Self::Edge(LayerEdgeStroke::parse(text)?),
            "crop" | "canvas_size" | "straighten" | "rotate90" | "rotate180" | "rotate270"
            | "flip_horizontal" | "flip_vertical" | "resize" | "affine" | "perspective"
            | "affine_set" | "perspective_set" => Self::Geometry(GeometryEdit::parse(text)?),
            op @ "move_selected" => {
                let edit = SelectedPixelsEdit::parse(text)?;
                if edit.target() != SelectedTarget::Layer {
                    return Err(refused_sentence(op));
                }
                Self::Selected(edit)
            }
            op => return Err(refused_sentence(op)),
        })
    }

    /// Whether the op paints an ink (the ink snap, module doc): the erasers,
    /// the geometry, the selected pixels' edits, Perspective Crop and the
    /// Clone Stamp carry none, and the Gradient's stops are prepared by the
    /// inner op and thresholded, not snapped.
    fn has_ink(&self) -> bool {
        !matches!(
            self,
            Self::Native(NativePaintEdit::EraseNative { .. } | NativePaintEdit::Gradient { .. })
                | Self::Legacy(PaintEdit::Erase { .. })
                | Self::Geometry(_)
                | Self::Selected(_)
                | Self::PerspectiveCrop(_)
                | Self::Clone(_)
        )
    }
}

impl BitmapEdit {
    /// An admitted request (`bitmap_document::admit_edit` asks first).
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let (stripped, background) = split_background(text)?;
        let op = BitmapOp::parse(&stripped)?;
        let request = serde_json::from_str(&stripped)
            .map_err(|e| format!("invalid high-depth edit request: {e}"))?;
        Ok(Self {
            request,
            op,
            background,
        })
    }

    /// The inner op's plan on the Gray storage with the adapter's bytes
    /// added to `scratch_bytes` (module doc). A scalar op's input is the
    /// canvas, one plane.
    pub(crate) fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        doc.bitmap_policy().ok_or(NOT_BITMAP)?;
        let gray = doc.without_bitmap();
        let mut output = (doc.width, doc.height);
        let mut value = match &self.op {
            BitmapOp::Native(edit) => edit.plan(&gray)?,
            BitmapOp::Edge(edit) => edit.plan(&gray)?,
            BitmapOp::Legacy(edit) => {
                let p = gray.paint_plan(edit)?;
                let mut value = json!({"retained_bytes":p.retained_bytes,"scratch_bytes":p.scratch_bytes,"input_samples":p.input_samples});
                if p.input_samples > 0 {
                    value["input_width"] = json!(doc.width);
                    value["input_height"] = json!(doc.height);
                    value["input_planes"] = json!(1);
                }
                value
            }
            BitmapOp::Geometry(edit) => {
                let p = gray.geometry_plan(edit)?;
                output = (p.width, p.height);
                json!({"retained_bytes":p.retained_bytes,"scratch_bytes":p.scratch_bytes,"input_samples":0,"work_units":p.work_units})
            }
            BitmapOp::Selected(edit) => {
                let p = gray.selected_pixels_plan(edit)?;
                let mut value = json!({"retained_bytes":p.retained_bytes,"scratch_bytes":p.scratch_bytes,"input_samples":p.input_samples});
                if p.input_samples > 0 {
                    value["input_width"] = json!(doc.width);
                    value["input_height"] = json!(doc.height);
                    value["input_planes"] = json!(1);
                }
                value
            }
            BitmapOp::PerspectiveCrop(edit) => {
                output = (edit.width, edit.height);
                gray.perspective_crop_plan(edit)?
            }
            BitmapOp::Clone(edit) => {
                let p = gray.retouch_plan(edit)?;
                let mut value = json!({"retained_bytes":p.retained_bytes,"scratch_bytes":p.scratch_bytes,"input_samples":p.input_samples});
                if p.input_samples > 0 {
                    value["input_width"] = json!(doc.width);
                    value["input_height"] = json!(doc.height);
                    value["input_planes"] = json!(if edit.selection { 2 } else { 1 });
                }
                value
            }
        };
        let scratch = value["scratch_bytes"]
            .as_u64()
            .ok_or("bitmap edit plan has no scratch_bytes")?;
        let pixels = u64::from(output.0) * u64::from(output.1);
        value["scratch_bytes"] = json!(scratch
            .saturating_add(pixels.saturating_mul(BITMAP_EDIT_BYTES_PER_PIXEL))
            .saturating_add(BOOKKEEPING_BYTES));
        Ok(value)
    }

    /// The background colour's bit (module doc): a Straighten's own
    /// `background`, else the request's key, else white, read on the Gray
    /// storage by `background_codes` and snapped at full alpha.
    fn background_white(&self, gray: &PrecisionDocument) -> Result<bool, String> {
        let own = match &self.op {
            BitmapOp::Geometry(GeometryEdit::Straighten { background, .. }) => background.as_ref(),
            _ => None,
        };
        let [gray_code, ..] = background_codes(gray, own.or(self.background.as_ref()))?;
        Ok(white_over(gray_code, 255, true))
    }

    /// The op with its ink snapped to black or white (module doc); the op
    /// as parsed when it carries no ink.
    fn snapped(&self, gray: &PrecisionDocument) -> Result<Option<BitmapOp>, String> {
        if !self.op.has_ink() {
            return Ok(None);
        }
        let color: [f32; 4] = serde_json::from_value(self.request["color"].clone())
            .map_err(|e| format!("invalid Bitmap paint colour: {e}"))?;
        let space: PaintColorSpace = serde_json::from_value(self.request["color_space"].clone())
            .map_err(|e| format!("invalid Bitmap paint colour space: {e}"))?;
        let prepared = crate::precision_paint::prepare_colors(&[color], space, gray)?;
        let prepared = prepared.first().ok_or("bitmap paint has no colour")?;
        let code = (f64::from(prepared[0]).clamp(0.0, 1.0) * 255.0).round() as u8;
        let v = if white_over(code, 255, true) {
            1.0
        } else {
            0.0
        };
        let mut request = self.request.clone();
        request["color"] = json!([v, v, v, color[3]]);
        request["color_space"] = json!("document");
        BitmapOp::parse(&request.to_string()).map(Some)
    }

    /// The edit (module doc); `Ok(None)` when no bit moves.
    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        doc.bitmap_policy().ok_or(NOT_BITMAP)?;
        let scratch = self.plan(doc)?["scratch_bytes"]
            .as_u64()
            .ok_or("bitmap edit plan has no scratch_bytes")?;
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let gray = doc.without_bitmap();
        let background_white = self.background_white(&gray)?;
        let snapped = self.snapped(&gray)?;
        let op = snapped.as_ref().unwrap_or(&self.op);
        let inner = job.range(0.0, 0.8).with_reservation_floor(scratch);
        let result = match op {
            BitmapOp::Native(edit) => edit.apply(&gray, input, limit, other, inner)?,
            BitmapOp::Legacy(edit) => gray.paint_with_context(edit, input, limit, other, inner)?,
            BitmapOp::Edge(edit) => edit.apply(&gray, input, limit, other, inner)?,
            BitmapOp::Geometry(edit) => gray.geometry_with_context(edit, limit, other, inner)?,
            BitmapOp::Selected(edit) => {
                gray.selected_pixels_with_context(edit, input, limit, other, inner)?
            }
            BitmapOp::PerspectiveCrop(edit) => gray.perspective_crop(edit, limit, other, inner)?,
            BitmapOp::Clone(edit) => gray.retouch_with_context(edit, input, limit, other, inner)?,
        };
        let Some(result) = result else {
            return Ok(None);
        };
        republish(doc, &result, background_white, true, job.range(0.8, 1.0))
    }
}

/// Edit ▸ Cut on a Bitmap document (module doc): the Gray storage's Cut,
/// then the erased layer republished, so each cut pixel (alpha 0) takes the
/// background bit (`background`, else white, snapped at full alpha),
/// admitted at the republish's bytes beside the payload and its encoding.
/// The job control is begun here, so the Gray Cut runs uncontrolled and the
/// republish under it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cut(
    doc: &PrecisionDocument,
    layer: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    coverage: Option<&[u16]>,
    background: Option<&CropBackground>,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<(ClipboardPayload, Option<PrecisionDocument>), String> {
    doc.bitmap_policy().ok_or(NOT_BITMAP)?;
    let gray = doc.without_bitmap();
    let [gray_code, ..] = background_codes(&gray, background)?;
    let background_white = white_over(gray_code, 255, true);
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let (payload, erased) =
        gray.clipboard_cut(layer, x, y, width, height, coverage, limit, other, None)?;
    let Some(erased) = erased else {
        return Ok((payload, None));
    };
    let held = other
        .checked_add(2 * payload.payload_bytes())
        .ok_or("clipboard size overflow")?;
    let scratch = (u64::from(doc.width) * u64::from(doc.height))
        .saturating_mul(BITMAP_EDIT_BYTES_PER_PIXEL)
        .saturating_add(BOOKKEEPING_BYTES);
    admit(doc.retained_bytes(), scratch, held, limit)?;
    job.reserve(scratch)?;
    let out = republish(doc, &erased, background_white, true, job)?;
    Ok((payload, out))
}

/// `result` (an op run on `doc`'s Gray storage) back on the plane (module
/// doc): each canvas pixel of its one layer by `white_over` over the
/// background (white when `background_white`, else black), then the layer
/// re-rendered and the document rebuilt. `None` when `compare` and the
/// canvas, the plane, the resolution and the chrome (guides, ruler origin,
/// colour samplers, paths and comps, which the Gray geometry kernel counts
/// as a change) are unchanged.
pub(crate) fn republish(
    doc: &PrecisionDocument,
    result: &PrecisionDocument,
    background_white: bool,
    compare: bool,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    let policy = doc.bitmap_policy().ok_or(NOT_BITMAP)?;
    let [layer] = result.root_layers() else {
        return Err("a Bitmap document holds one layer".into());
    };
    let Content::Raster { image, offset } = &layer.content else {
        return Err("a Bitmap document's layer is a raster".into());
    };
    let (width, height) = result.dimensions();
    let line = stride(width);
    let mut packed = Vec::new();
    packed
        .try_reserve_exact(line * height as usize)
        .map_err(|_| "the Bitmap plane could not be allocated")?;
    let mut wide = vec![[0u16; 4]; width as usize];
    let mut bits = vec![0u8; width as usize];
    let mut row = vec![0u8; line];
    let rows = job.range(0.0, 0.5);
    for y in 0..height {
        canvas_row(image, *offset, y, &mut wide)?;
        for (bit, pixel) in bits.iter_mut().zip(&wide) {
            *bit = u8::from(white_over(
                code8(pixel[0]),
                code8(pixel[3]),
                background_white,
            ));
        }
        pack_row(&bits, &mut row);
        packed.extend_from_slice(&row);
        rows.report(f64::from(y + 1) / f64::from(height), 5)?;
    }
    if compare
        && policy.dimensions() == (width, height)
        && packed.as_slice() == policy.packed()
        && result.resolution() == doc.resolution()
        && result.guides() == doc.guides()
        && result.ruler_origin() == doc.ruler_origin()
        && result.color_samplers() == doc.color_samplers()
        && result.paths() == doc.paths()
        && result.comps == doc.comps
    {
        return Ok(None);
    }
    let next = BitmapPolicy::new(packed, width, height)?;
    let image = next.render(result.profile(), job.range(0.5, 0.98))?;
    let mut layer = layer.clone();
    layer.content = Content::Raster {
        image,
        offset: (0, 0),
    };
    layer.warp = None;
    let out = result
        .without_bitmap()
        .rebuilt(width, height, vec![layer], result.channels().to_vec())?
        .with_bitmap(Arc::new(next))?;
    job.report(1.0, 5)?;
    Ok(Some(out))
}
