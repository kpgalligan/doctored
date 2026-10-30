//! The paint family on an Indexed Color document (#1 Indexed phases 2b
//! and 2c, `designs/indexed-mode.md`): each op runs on the
//! document's RGB storage through its existing kernel, then the result is
//! re-indexed by the edge rule (`indexed_edge`) against the plane from
//! before it.
//!
//! The ops (`indexed_document::admit_edit` routes them here): the Brush and
//! Fill (`paint_blend`), the Paint Bucket (`bucket_fill`) and Stroke… with
//! nothing selected (`stroke_layer_edge`), in Normal mode (phase 3b: every
//! mode, below), each with a `Fixed` ink: the request's colour as the paint itself prepares it
//! (`precision_paint::prepare_colors`), at 8-bit codes; Fill with Preserve
//! and the scalar paint (`paint`, through `paint_with_context`) the same;
//! the Eraser (`erase_native`), Clear (`erase`) and Cut ([`cut`]) with the
//! `Clear` ink, which keeps each index whose pixel stays at alpha 128 or
//! above and gives the rest `indexed_edge::clear_index`: the transparent
//! index, or with none (since phase 3a) the entry nearest the background
//! colour, as a Background layer erases.
//!
//! [`republish`] is the one way back: the canvas window of the result's one
//! layer (read through its offset, so a stroke that grew the layer past the
//! canvas is clipped to it, pixels outside the layer clear), re-indexed;
//! a plane and canvas equal to the old ones are `None` (purity: an edit
//! too faint to move any index changes nothing). Otherwise the one "Index"
//! layer is re-rendered from the new policy and the document rebuilt as
//! `indexed_table`'s Color Table edit rebuilds it (the edited record kept,
//! its content replaced, at the origin), so the layer is exactly the
//! policy's render and the `.rz` writer's `indexed_native::check` accepts
//! it. The resampling geometry (`indexed_geometry`) republishes through it
//! too, with no backdrop.
//!
//! Since phase 3b the Brush, Fill, Stroke and the Paint Bucket paint in
//! every blend mode they take on RGB: a mode other than Normal re-indexes
//! with `EdgeInk::Blend` (`indexed_edge`'s module doc), Normal with
//! `Fixed` as before. And every request this module, `indexed_geometry` and
//! `indexed_transform` take, Cut and a warp `set` may carry
//! [`BACKGROUND_KEY`], the colour a clear pixel's entry is nearest with no
//! transparent index (the host's background swatch, in `CropBackground`'s
//! shape, prepared as `doc_crop_extend` prepares a crop's Background by
//! [`background_codes`]); absent, the colour is white
//! (`indexed_edge::BACKGROUND`). [`split_background`] takes the key off
//! before the inner kernel's strict parser reads the request.
//!
//! Since phase 3c Clone Stamp (`retouch_clone`, in place on the one layer)
//! and the History Brush ([`history_brush`], asked by
//! `precision_retouch_brush`) run the same way with the `Own` ink against
//! the plane from before: a stroke moves each covered pixel to the entry
//! nearest the colour it lands on, so a hard, full-opacity clone copies the
//! source entries and a full History Brush stroke restores the earlier
//! plane where the earlier state's colours are entries. A clone onto a new
//! layer refuses as Layer ▸ New does.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::blend::BlendMode;
use crate::indexed_document::{refused_sentence, IndexedPolicy};
use crate::indexed_edge::{reindex, EdgeInk, EdgeInput, BACKGROUND};
use crate::indexed_palette::{code8, Rgb, MEMO_BYTES};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_clipboard::ClipboardPayload;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_fill_stroke::LayerEdgeStroke;
use crate::precision_geometry::{CropBackground, CROP_BACKGROUND_SHAPE};
use crate::precision_image::{admit, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES};
use crate::precision_paint::{PaintColorSpace, PaintEdit};
use crate::precision_paint_tools::NativePaintEdit;
use crate::precision_retouch::RetouchEdit;

/// The request key that names the background colour (module doc).
pub(crate) const BACKGROUND_KEY: &str = "indexed_background";

/// `text` with [`BACKGROUND_KEY`] removed and its value, `None` when the
/// request names none (module doc). The value is a `CropBackground` with a
/// `color`: an `ink` refuses with the crop's shape sentence.
pub(crate) fn split_background(text: &str) -> Result<(String, Option<CropBackground>), String> {
    split_background_as(text, "invalid high-depth edit request")
}

/// [`split_background`] with `invalid` as the prefix of a JSON error (Cut's
/// own, `ffi_clipboard`).
pub(crate) fn split_background_as(
    text: &str,
    invalid: &str,
) -> Result<(String, Option<CropBackground>), String> {
    let mut value: Value = serde_json::from_str(text).map_err(|e| format!("{invalid}: {e}"))?;
    let Some(found) = value.as_object_mut().and_then(|o| o.remove(BACKGROUND_KEY)) else {
        return Ok((text.to_string(), None));
    };
    let background: CropBackground = serde_json::from_value(found)
        .map_err(|e| format!("invalid Indexed Color background: {e}"))?;
    if background.ink.is_some() {
        return Err(CROP_BACKGROUND_SHAPE.into());
    }
    Ok((value.to_string(), Some(background)))
}

/// The background colour at 8-bit codes of `rgb` (the RGB storage): white
/// with no `background`, else its `color` prepared from its `color_space`
/// (`document` unless named) as `doc_crop_extend` prepares a crop's
/// Background, the one preparation of the Indexed background colour.
pub(crate) fn background_codes(
    rgb: &PrecisionDocument,
    background: Option<&CropBackground>,
) -> Result<Rgb, String> {
    let Some(background) = background else {
        return Ok(BACKGROUND);
    };
    let color = match (background.color, background.ink) {
        (Some(color), None) if color.iter().all(|v| v.is_finite()) => color,
        _ => return Err(CROP_BACKGROUND_SHAPE.into()),
    };
    let space = background.color_space.unwrap_or(PaintColorSpace::Document);
    let color = color.map(|v| v.clamp(0.0, 1.0) as f32);
    let prepared =
        crate::precision_paint::prepare_colors(&[[color[0], color[1], color[2], 1.0]], space, rgb)?;
    let stored = crate::precision_paint::store(*prepared.first().ok_or("no background colour")?);
    Ok([stored[0], stored[1], stored[2]].map(code8))
}

/// The re-index's working memory per canvas pixel, beside the op's own:
/// the new plane (1), the re-rendered RGBA16 layer (8) and the row of RGBA8
/// codes and its slack (1). Declared before measuring
/// (`scripts/indexed-admission/measure.py`, case `brush-indexed`).
pub(crate) const INDEXED_PAINT_BYTES_PER_PIXEL: u64 = 10;

/// The refusal on any document but an Indexed one.
pub(crate) const NOT_INDEXED: &str = "the document is not Indexed Color";

/// One paint-family edit on an Indexed document (module doc).
pub(crate) struct IndexedPaintEdit {
    op: IndexedPaintOp,
    /// The request's `color` and `color_space`; `None` for the erasers,
    /// whose ink is `EdgeInk::Clear`.
    color: Option<([f32; 4], PaintColorSpace)>,
    /// The request's `blend_mode` when it names one but Normal (phase 3b):
    /// the ink is then `EdgeInk::Blend`.
    mode: Option<BlendMode>,
    /// The request's [`BACKGROUND_KEY`] (module doc).
    background: Option<CropBackground>,
}

/// The existing kernel an [`IndexedPaintEdit`] runs.
enum IndexedPaintOp {
    Native(NativePaintEdit),
    Legacy(PaintEdit),
    Edge(LayerEdgeStroke),
    /// Clone Stamp (`retouch_clone` only, phase 3c): the `Own` ink.
    Clone(RetouchEdit),
}

/// The re-index's bytes beside an op's own (module doc): the per-pixel
/// shape over a `width`×`height` canvas, the nearest-entry memo E is looked
/// up through and bookkeeping.
pub(crate) fn reindex_bytes(width: u32, height: u32) -> u64 {
    let canvas = u64::from(width) * u64::from(height);
    canvas
        .saturating_mul(INDEXED_PAINT_BYTES_PER_PIXEL)
        .saturating_add(MEMO_BYTES)
        .saturating_add(BOOKKEEPING_BYTES)
}

impl IndexedPaintEdit {
    /// A `paint_blend`, `bucket_fill`, `erase_native`, `paint`, `erase`,
    /// `stroke_layer_edge` or (phase 3c) `retouch_clone` request; every other
    /// op refuses with the mode's sentence (`indexed_document::admit_edit`
    /// asks first). A clone whose `output` is a new layer refuses as
    /// `add_layer` does.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let (stripped, background) = split_background(text)?;
        let text = stripped.as_str();
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("invalid high-depth edit request: {e}"))?;
        let op = value["op"].as_str().unwrap_or("edit");
        let op = match op {
            "paint_blend" | "bucket_fill" | "erase_native" => {
                IndexedPaintOp::Native(NativePaintEdit::parse(text)?)
            }
            "paint" | "erase" => IndexedPaintOp::Legacy(PaintEdit::parse(text)?),
            "stroke_layer_edge" => IndexedPaintOp::Edge(LayerEdgeStroke::parse(text)?),
            "retouch_clone" => {
                if value["output"].as_str() == Some("new_layer") {
                    return Err(refused_sentence("add_layer"));
                }
                IndexedPaintOp::Clone(RetouchEdit::parse(text)?)
            }
            _ => return Err(refused_sentence(op)),
        };
        if let IndexedPaintOp::Clone(_) = op {
            return Ok(Self {
                op,
                color: None,
                mode: None,
                background,
            });
        }
        // An unknown or group-only mode is the kernel's refusal (it runs
        // first); Normal, absent or unknown here is the `Fixed` ink.
        let mode = value["blend_mode"]
            .as_i64()
            .and_then(|m| i32::try_from(m).ok())
            .and_then(BlendMode::from_c)
            .filter(|m| !matches!(m, BlendMode::Normal | BlendMode::PassThrough));
        let color = match &op {
            IndexedPaintOp::Native(NativePaintEdit::EraseNative { .. })
            | IndexedPaintOp::Legacy(PaintEdit::Erase { .. }) => None,
            _ => Some((
                serde_json::from_value(value["color"].clone())
                    .map_err(|e| format!("invalid Indexed Color paint colour: {e}"))?,
                serde_json::from_value(value["color_space"].clone())
                    .map_err(|e| format!("invalid Indexed Color paint colour space: {e}"))?,
            )),
        };
        Ok(Self {
            op,
            color,
            mode,
            background,
        })
    }

    /// The op's plan on the RGB storage with the re-index's bytes added to
    /// `scratch_bytes`. A scalar op's input is the canvas, one plane.
    pub(crate) fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        doc.indexed_policy().ok_or(NOT_INDEXED)?;
        let rgb = doc.without_indexed();
        let mut value = match &self.op {
            IndexedPaintOp::Native(edit) => edit.plan(&rgb)?,
            IndexedPaintOp::Edge(edit) => edit.plan(&rgb)?,
            IndexedPaintOp::Legacy(edit) => {
                let p = rgb.paint_plan(edit)?;
                let mut value = json!({"retained_bytes":p.retained_bytes,"scratch_bytes":p.scratch_bytes,"input_samples":p.input_samples});
                if p.input_samples > 0 {
                    value["input_width"] = json!(doc.width);
                    value["input_height"] = json!(doc.height);
                    value["input_planes"] = json!(1);
                }
                value
            }
            IndexedPaintOp::Clone(edit) => {
                let p = rgb.retouch_plan(edit)?;
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
            .ok_or("indexed paint plan has no scratch_bytes")?;
        value["scratch_bytes"] =
            json!(scratch.saturating_add(reindex_bytes(doc.width, doc.height)));
        Ok(value)
    }

    /// The edit's ink (module doc), prepared against the RGB storage.
    fn ink(&self, rgb: &PrecisionDocument) -> Result<EdgeInk, String> {
        if let IndexedPaintOp::Clone(_) = self.op {
            return Ok(EdgeInk::Own);
        }
        let Some((color, space)) = self.color else {
            return Ok(EdgeInk::Clear);
        };
        let prepared = crate::precision_paint::prepare_colors(&[color], space, rgb)?;
        let color = prepared.first().ok_or("indexed paint has no colour")?;
        let ink = [0, 1, 2].map(|c| (f64::from(color[c]).clamp(0.0, 1.0) * 255.0).round() as u8);
        Ok(match self.mode {
            Some(mode) => EdgeInk::Blend(ink, mode),
            None => EdgeInk::Fixed(ink),
        })
    }

    /// The edit (module doc); `Ok(None)` when no index moves.
    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        doc.indexed_policy().ok_or(NOT_INDEXED)?;
        let scratch = self.plan(doc)?["scratch_bytes"]
            .as_u64()
            .ok_or("indexed paint plan has no scratch_bytes")?;
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let rgb = doc.without_indexed();
        let inner = job.range(0.0, 0.8).with_reservation_floor(scratch);
        let result = match &self.op {
            IndexedPaintOp::Native(edit) => edit.apply(&rgb, input, limit, other, inner)?,
            IndexedPaintOp::Legacy(edit) => {
                rgb.paint_with_context(edit, input, limit, other, inner)?
            }
            IndexedPaintOp::Edge(edit) => edit.apply(&rgb, input, limit, other, inner)?,
            IndexedPaintOp::Clone(edit) => {
                rgb.retouch_with_context(edit, input, limit, other, inner)?
            }
        };
        let Some(result) = result else {
            return Ok(None);
        };
        let ink = self.ink(&rgb)?;
        let background = background_codes(&rgb, self.background.as_ref())?;
        republish(doc, &result, ink, true, background, job.range(0.8, 1.0))
    }
}

/// Row `y` of the canvas window of `image` placed at `offset` into `buf`
/// (the canvas width), RGBA16; pixels outside the image are 0.
pub(crate) fn canvas_row(
    image: &PrecisionImage,
    offset: (i32, i32),
    y: u32,
    buf: &mut [[u16; 4]],
) -> Result<(), String> {
    buf.fill([0; 4]);
    let (iw, ih) = image.dimensions();
    let ly = i64::from(y) - i64::from(offset.1);
    if ly < 0 || ly >= i64::from(ih) {
        return Ok(());
    }
    let ox = i64::from(offset.0);
    let start = (-ox).max(0);
    let end = i64::from(iw).min(buf.len() as i64 - ox);
    if start >= end {
        return Ok(());
    }
    for (x, run) in image.row_segments(ly as u32, start as u32, end as u32) {
        let PrecisionRun::Rgba16(run) = run else {
            return Err("an Indexed Color document's layer is 16-bit RGBA".into());
        };
        let at = (i64::from(x) + ox) as usize;
        buf[at..at + run.len()].copy_from_slice(run);
    }
    Ok(())
}

/// `result` (the op run on `doc`'s RGB storage) back on `doc`'s palette
/// (module doc): re-indexed with `ink`, against `doc`'s plane when
/// `compare` (else every pixel is changed, a resample's rule), a clear
/// pixel with no transparent index taking the entry nearest `background`,
/// then re-rendered and rebuilt. `None` when the canvas and the plane are
/// unchanged.
pub(crate) fn republish(
    doc: &PrecisionDocument,
    result: &PrecisionDocument,
    ink: EdgeInk,
    compare: bool,
    background: Rgb,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    let policy = doc.indexed_policy().ok_or(NOT_INDEXED)?;
    let [layer] = result.root_layers() else {
        return Err("an Indexed Color document holds one layer".into());
    };
    let Content::Raster { image, offset } = &layer.content else {
        return Err("an Indexed Color document's layer is a raster".into());
    };
    let (width, height) = result.dimensions();
    let before = if compare {
        if policy.dimensions() != (width, height) {
            return Err("the Indexed Color index plane does not match the canvas".into());
        }
        Some(policy.indices())
    } else {
        None
    };
    let input = EdgeInput {
        palette: policy.palette(),
        transparent: policy.transparent(),
        width,
        height,
        before,
        ink,
        background,
    };
    let rows = job.range(0.0, 0.5);
    let mut wide = vec![[0u16; 4]; width as usize];
    let plane = reindex(&input, |y, buf| {
        canvas_row(image, *offset, y, &mut wide)?;
        for (dst, pixel) in buf.iter_mut().zip(&wide) {
            *dst = pixel.map(code8);
        }
        // Once a row: `every` on a row start would poll only when `y·width`
        // is a multiple of its interval, rarely on an odd width.
        rows.report(f64::from(y + 1) / f64::from(height), 5)
    })?;
    if policy.dimensions() == (width, height) && plane.as_slice() == policy.indices() {
        return Ok(None);
    }
    let next = IndexedPolicy::new(
        policy.palette().to_vec(),
        policy.transparent(),
        plane,
        width,
        height,
    )?;
    rebuild(result, next, job.range(0.5, 1.0)).map(Some)
}

/// `result` with its one layer replaced by `policy`'s render at the origin
/// and the policy attached (2b's rebuild, module doc). The layer keeps no
/// warp binding (phase 2d): an Index layer never carries one, so a warp is
/// applied at once.
pub(crate) fn rebuild(
    result: &PrecisionDocument,
    policy: IndexedPolicy,
    job: JobContext<'_>,
) -> Result<PrecisionDocument, String> {
    let [layer] = result.root_layers() else {
        return Err("an Indexed Color document holds one layer".into());
    };
    let (width, height) = result.dimensions();
    let image = policy.render(result.profile(), job.range(0.0, 0.98))?;
    let mut layer = layer.clone();
    layer.content = Content::Raster {
        image,
        offset: (0, 0),
    };
    layer.warp = None;
    let out = result
        .without_indexed()
        .rebuilt(width, height, vec![layer], result.channels().to_vec())?
        .with_indexed(Arc::new(policy))?;
    job.report(1.0, 5)?;
    Ok(out)
}

/// Edit ▸ Cut on an Indexed document (module doc): the RGB storage's Cut,
/// then the erased layer republished with the `Clear` ink (a cut pixel takes
/// `indexed_edge::clear_index`, with no transparent index the entry nearest
/// `background`, [`background_codes`]), admitted at the re-index's
/// bytes beside the payload and its encoding. The job control is begun
/// here, so the RGB Cut runs uncontrolled and the re-index under it.
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
    doc.indexed_policy().ok_or(NOT_INDEXED)?;
    let rgb = doc.without_indexed();
    let background = background_codes(&rgb, background)?;
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let (payload, erased) =
        rgb.clipboard_cut(layer, x, y, width, height, coverage, limit, other, None)?;
    let Some(erased) = erased else {
        return Ok((payload, None));
    };
    let held = other
        .checked_add(2 * payload.payload_bytes())
        .ok_or("clipboard size overflow")?;
    let scratch = reindex_bytes(doc.width, doc.height);
    admit(doc.retained_bytes(), scratch, held, limit)?;
    job.reserve(scratch)?;
    let out = republish(doc, &erased, EdgeInk::Clear, true, background, job)?;
    Ok((payload, out))
}

/// The History Brush's plan on an Indexed document (phase 3c): the RGB
/// storage's plan with the re-index's bytes added to `scratch_bytes`.
pub(crate) fn history_brush_plan(
    doc: &PrecisionDocument,
    layer: usize,
    source: &PrecisionDocument,
    source_layer: usize,
    selection: bool,
) -> Result<Value, String> {
    doc.indexed_policy().ok_or(NOT_INDEXED)?;
    let mut value =
        doc.without_indexed()
            .history_brush_plan(layer, source, source_layer, selection)?;
    let scratch = value["scratch_bytes"]
        .as_u64()
        .ok_or("indexed history brush plan has no scratch_bytes")?;
    value["scratch_bytes"] = json!(scratch.saturating_add(reindex_bytes(doc.width, doc.height)));
    Ok(value)
}

/// The History Brush on an Indexed document (module doc): the RGB storage's
/// stroke, then the result re-indexed with the `Own` ink against the plane
/// from before. `Ok(None)` when no index moves.
#[allow(clippy::too_many_arguments)]
pub(crate) fn history_brush(
    doc: &PrecisionDocument,
    layer: usize,
    source: &PrecisionDocument,
    source_layer: usize,
    opacity: f64,
    samples: &[u16],
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    doc.indexed_policy().ok_or(NOT_INDEXED)?;
    let canvas = crate::precision_image::pixel_count(doc.width, doc.height)?;
    let selection = samples.len() == 2 * canvas;
    let scratch = history_brush_plan(doc, layer, source, source_layer, selection)?["scratch_bytes"]
        .as_u64()
        .ok_or("indexed history brush plan has no scratch_bytes")?;
    admit(doc.retained_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    let rgb = doc.without_indexed();
    let inner = job.range(0.0, 0.8).with_reservation_floor(scratch);
    let Some(result) = rgb.history_brush(
        layer,
        source,
        source_layer,
        opacity,
        samples,
        limit,
        other,
        inner,
    )?
    else {
        return Ok(None);
    };
    republish(
        doc,
        &result,
        EdgeInk::Own,
        true,
        BACKGROUND,
        job.range(0.8, 1.0),
    )
}
