//! Bounded internal 7D request bridge. Numeric kernels and target validation
//! stay in the typed feature modules. The byte document/overlay API is separate.
//! Requests are bounded before parsing; pixel counts come from core plans.
//!
//! The FFI owns one token from pre-parse admission through atomic scalar-output publication.

use std::ffi::{c_char, CString};
use std::ptr;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_geometry::GeometryEdit;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::precision_mask_edit::MaskEdit;
use crate::precision_paint::PaintEdit;
use crate::precision_plane_read::PlaneRead;
use crate::precision_structure::StructureEdit;

const REQUEST_LIMIT: usize = 64 * 1024;
/// A `layer_style` request alone may be larger: it carries a whole style,
/// whose canonical body may reach `MAX_STYLE_JSON_LEN` (2 MiB, for custom
/// pattern tiles), beside the envelope `REQUEST_LIMIT` bounds.
const STYLE_REQUEST_LIMIT: usize =
    crate::precision_style_native::MAX_STYLE_JSON_LEN + REQUEST_LIMIT;
const PARSE_BYTES: u64 = 4 * BOOKKEEPING_BYTES;

/// Whether `text` is a `layer_style` request: the one op admitted past
/// `REQUEST_LIMIT`. Reads only the `op` key, so nothing is allocated.
fn is_style_request(text: &str) -> bool {
    #[derive(Deserialize)]
    struct Tag<'a> {
        #[serde(borrow)]
        op: &'a str,
    }
    serde_json::from_str::<Tag<'_>>(text).is_ok_and(|tag| tag.op == "layer_style")
}

enum Edit {
    Geometry(GeometryEdit),
    PerspectiveCrop(crate::perspective_crop::PerspectiveCropRequest),
    Align(crate::precision_align::AlignEdit),
    Mask(MaskEdit),
    Channel(crate::precision_channel_edit::ChannelEdit),
    Paint(PaintEdit),
    NativePaint(crate::precision_paint_tools::NativePaintEdit),
    /// The same tools in ink on a CMYK document (`cmyk_paint`, #1 phase 2).
    CmykPaint(crate::cmyk_paint::CmykPaintEdit),
    /// The paint family on an Indexed document (`indexed_paint`, #1 Indexed
    /// phases 2b and 2c).
    IndexedPaint(crate::indexed_paint::IndexedPaintEdit),
    /// The whole-canvas geometry on an Indexed document (`indexed_geometry`,
    /// #1 Indexed phase 2c).
    IndexedGeometry(crate::indexed_geometry::IndexedGeometryEdit),
    /// The selected pixels' transform on an Indexed document
    /// (`indexed_transform`, #1 Indexed phase 2d).
    IndexedSelected(crate::indexed_transform::IndexedSelectedEdit),
    /// The paint family and the bit-exact geometry on a Bitmap document
    /// (`bitmap_edit`, #1 Bitmap phase 2a-i).
    Bitmap(crate::bitmap_edit::BitmapEdit),
    RetouchBrush(crate::precision_retouch_brush::RetouchBrushEdit),
    LayerEdgeStroke(crate::precision_fill_stroke::LayerEdgeStroke),
    /// Stroke… with nothing selected in ink on a CMYK document (`cmyk_stroke`, #1).
    CmykLayerEdgeStroke(crate::cmyk_stroke::CmykLayerEdgeStroke),
    Retouch(crate::precision_retouch::RetouchEdit),
    Read(PlaneRead),
    Structure(StructureEdit),
    Comp(crate::precision_comps::CompEdit),
    LayerBlend(crate::precision_layer_blend::LayerBlendEdit),
    BlendIf(crate::precision_blend_if::LayerBlendIfEdit),
    Style(crate::precision_style::LayerStyleEdit),
    LayerVia(crate::precision_layer_via::LayerViaEdit),
    StackMode(crate::stack_stat::StackModeRequest),
    Scalar(crate::precision_scalar_edit::ScalarEdit),
    ScalarStatistics(crate::precision_scalar_statistics::ScalarStatistics),
    Selection(crate::precision_selection::SelectionRequest),
    SelectedPixels(crate::selected_pixels::SelectedPixelsEdit),
    GlobalLight(crate::precision_global_light::GlobalLightEdit),
}

impl Edit {
    fn parse(text: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Tag<'a> {
            #[serde(borrow)]
            op: &'a str,
        }
        let tag: Tag<'_> = serde_json::from_str(text)
            .map_err(|e| format!("invalid high-depth edit request: {e}"))?;
        match tag.op {
            "affine" | "perspective" | "affine_set" | "perspective_set" | "resize" | "crop"
            | "canvas_size" | "straighten" | "rotate_canvas" | "rotate90" | "rotate180"
            | "rotate270" | "flip_horizontal" | "flip_vertical" => {
                GeometryEdit::parse(text).map(Self::Geometry)
            }
            "perspective_crop" => crate::perspective_crop::PerspectiveCropRequest::parse(text)
                .map(Self::PerspectiveCrop),
            "align_set" | "distribute_set" => {
                crate::precision_align::AlignEdit::parse(text).map(Self::Align)
            }
            "set_mask" | "remove_mask" | "apply_mask" | "mask_properties" | "set_channel"
            | "remove_channel" => MaskEdit::parse(text).map(Self::Mask),
            "rename_channel" | "duplicate_channel" | "channel_properties" | "import_channel" => {
                crate::precision_channel_edit::ChannelEdit::parse(text).map(Self::Channel)
            }
            "paint" | "erase" | "clone" | "heal" | "rasterize" => {
                PaintEdit::parse(text).map(Self::Paint)
            }
            "paint_blend" | "bucket_fill" | "gradient" | "dodge_burn" | "erase_native" => {
                crate::precision_paint_tools::NativePaintEdit::parse(text).map(Self::NativePaint)
            }
            "retouch_brush" => crate::precision_retouch_brush::RetouchBrushEdit::parse(text)
                .map(Self::RetouchBrush),
            "stroke_layer_edge" => crate::precision_fill_stroke::LayerEdgeStroke::parse(text)
                .map(Self::LayerEdgeStroke),
            "retouch_clone" | "retouch_heal" | "spot_heal" | "red_eye" | "content_aware_fill" => {
                crate::precision_retouch::RetouchEdit::parse(text).map(Self::Retouch)
            }
            "layer_blend" => {
                crate::precision_layer_blend::LayerBlendEdit::parse(text).map(Self::LayerBlend)
            }
            "layer_blend_if" => {
                crate::precision_blend_if::LayerBlendIfEdit::parse(text).map(Self::BlendIf)
            }
            "layer_style" => crate::precision_style::LayerStyleEdit::parse(text).map(Self::Style),
            "layer_via" => {
                crate::precision_layer_via::LayerViaEdit::parse(text).map(Self::LayerVia)
            }
            "stack_mode" => crate::stack_stat::StackModeRequest::parse(text).map(Self::StackMode),
            "rename_layer" | "layer_visible" | "layer_clipped" | "layer_label" | "layer_open"
            | "move_layer_to" | "arrange_layer" | "group_layers" | "ungroup_layer"
            | "remove_layers" | "merge_layers" | "merge_down" | "merge_visible" | "layer_lock"
            | "link_layers" | "unlink_layers" | "add_layer" | "stamp_visible" | "flatten" => {
                StructureEdit::parse(text).map(Self::Structure)
            }
            "stamp_selected" | "merge_group" => StructureEdit::parse(text).map(Self::Structure),
            "duplicate_to_new_document" => StructureEdit::parse(text).map(Self::Structure),
            "layer_comp" => crate::precision_comps::CompEdit::parse(text).map(Self::Comp),
            "global_light" => {
                crate::precision_global_light::GlobalLightEdit::parse(text).map(Self::GlobalLight)
            }
            "read_plane" => PlaneRead::parse(text).map(Self::Read),
            "scalar_edit" => {
                crate::precision_scalar_edit::ScalarEdit::parse(text).map(Self::Scalar)
            }
            "scalar_statistics" => {
                crate::precision_scalar_statistics::ScalarStatistics::parse(text)
                    .map(Self::ScalarStatistics)
            }
            "selection" => {
                crate::precision_selection::SelectionRequest::parse(text).map(Self::Selection)
            }
            "move_selected" | "transform_selected" | "puppet_selected" | "perspective_selected" => {
                crate::selected_pixels::SelectedPixelsEdit::parse(text).map(Self::SelectedPixels)
            }
            _ => Err("operation is unavailable in the high-depth editing request API".into()),
        }
    }
    fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        let mut value = match self {
            Self::Geometry(edit) => {
                let p = doc.geometry_plan(edit)?;
                json!({"retained_bytes":p.retained_bytes,"scratch_bytes":p.scratch_bytes,"input_samples":0,"work_units":p.work_units})
            }
            Self::PerspectiveCrop(edit) => doc.perspective_crop_plan(edit)?,
            Self::Align(edit) => {
                let p = doc.align_plan(edit)?;
                json!({"retained_bytes":p.retained_bytes,"scratch_bytes":p.scratch_bytes,"input_samples":0})
            }
            Self::Mask(edit) => {
                json!({"retained_bytes":doc.retained_bytes(),"scratch_bytes":edit.scratch_bytes(doc)?,"input_samples":edit.expected_samples(doc)?})
            }
            Self::Channel(edit) => {
                json!({"retained_bytes":doc.retained_bytes(),"scratch_bytes":edit.scratch_bytes(doc)?,"input_samples":edit.expected_samples(doc)?})
            }
            Self::Paint(edit) => {
                let p = doc.paint_plan(edit)?;
                json!({"retained_bytes":p.retained_bytes,"scratch_bytes":p.scratch_bytes,"input_samples":p.input_samples})
            }
            Self::NativePaint(edit) => edit.plan(doc)?,
            Self::CmykPaint(edit) => edit.plan(doc)?,
            Self::IndexedPaint(edit) => edit.plan(doc)?,
            Self::IndexedGeometry(edit) => edit.plan(doc)?,
            Self::IndexedSelected(edit) => edit.plan(doc)?,
            Self::Bitmap(edit) => edit.plan(doc)?,
            Self::RetouchBrush(edit) => edit.plan(doc)?,
            Self::LayerEdgeStroke(edit) => edit.plan(doc)?,
            Self::CmykLayerEdgeStroke(edit) => edit.plan(doc)?,
            Self::Retouch(edit) => {
                let p = doc.retouch_plan(edit)?;
                json!({"retained_bytes":p.retained_bytes,"scratch_bytes":p.scratch_bytes,"input_samples":p.input_samples})
            }
            Self::Read(read) => read.plan(doc)?,
            Self::Scalar(edit) => edit.plan(doc)?,
            Self::ScalarStatistics(read) => read.plan(doc)?,
            Self::Selection(edit) => edit.plan(doc)?,
            Self::LayerBlend(edit) => {
                json!({"retained_bytes":doc.retained_bytes(), "scratch_bytes":edit.scratch_bytes(doc)?, "input_samples":0})
            }
            Self::BlendIf(edit) => {
                json!({"retained_bytes":doc.retained_bytes(), "scratch_bytes":edit.scratch_bytes(doc)?, "input_samples":0})
            }
            Self::Style(edit) => {
                json!({"retained_bytes":doc.retained_bytes(), "scratch_bytes":edit.scratch_bytes(doc)?, "input_samples":0})
            }
            Self::LayerVia(edit) => edit.plan(doc)?,
            Self::StackMode(edit) => doc.stack_mode_plan(edit)?,
            Self::Structure(edit) => {
                // A structural edit's plan carries the REPORT the two
                // grouping ops owe their caller — which entries lost a
                // clipping mask, which changed relative order, where the
                // freed children landed — so the host reads it before it
                // decides to apply.
                let mut value = json!({"retained_bytes":doc.retained_bytes(), "scratch_bytes":edit.scratch_bytes(doc)?, "input_samples":0});
                if let Some(report) = edit.report(doc)? {
                    value["report"] = report;
                }
                value
            }
            Self::Comp(edit) => {
                json!({"retained_bytes":doc.retained_bytes(), "scratch_bytes":edit.scratch_bytes(doc)?, "input_samples":0})
            }
            Self::GlobalLight(edit) => {
                json!({"retained_bytes":doc.retained_bytes(), "scratch_bytes":edit.scratch_bytes(doc)?, "input_samples":0})
            }
            Self::SelectedPixels(edit) => doc.selected_pixels_plan(edit)?.json(),
        };
        if !matches!(
            self,
            Self::Scalar(_)
                | Self::NativePaint(_)
                | Self::CmykPaint(_)
                | Self::IndexedPaint(_)
                | Self::Bitmap(_)
                | Self::RetouchBrush(_)
        ) && value["input_samples"].as_u64().is_some_and(|n| n > 0)
        {
            let (width, height, planes) = match self {
                Self::Mask(MaskEdit::SetMask { width, height, .. }) => (*width, *height, 1),
                Self::Channel(crate::precision_channel_edit::ChannelEdit::ImportChannel {
                    width,
                    height,
                    ..
                }) => (*width, *height, 1),
                Self::Paint(PaintEdit::Rasterize {
                    width,
                    height,
                    colors,
                    ..
                }) => (*width, *height, colors.len()),
                Self::Retouch(edit) if edit.selection => {
                    let (w, h) = doc.dimensions();
                    (w, h, 2)
                }
                _ => {
                    let (w, h) = doc.dimensions();
                    (w, h, 1)
                }
            };
            value["input_width"] = json!(width);
            value["input_height"] = json!(height);
            value["input_planes"] = json!(planes);
        }
        Ok(value)
    }
}

unsafe fn request<'a>(
    document: *const PrecisionDocument,
    text: *const c_char,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<(&'a PrecisionDocument, Edit, u64), String> {
    // The returned borrow is used only within each FFI closure. Handle lifetime
    // remains the caller's contract, exactly as required_handle elsewhere.
    let doc = unsafe { required_handle(document) }?;
    admit(doc.retained_bytes(), PARSE_BYTES, other, limit)?;
    job.reserve(PARSE_BYTES)?;
    let text = unsafe { borrow_cstr_bounded(text, "request", STYLE_REQUEST_LIMIT) }?;
    // The parsed tree of a request past the common bound (the style's
    // `Value`, then its decoded tile) is charged at four times the request's
    // length beside the bookkeeping, before any of it exists. The caller
    // raises the plan's scratch to this figure (`covering_parse`), so the
    // host's job limit, set from the plan, admits this reservation.
    // Measured (`precision_pattern_overlay::measure_layer_style_edit`,
    // Release, one case per process, `MallocLargeCache=0`, 3 runs and a
    // small-style twin at E ≤ 81,920): a 512² tile's 1,398,255-byte request
    // peaked at E 1,490,968 against a plan of 9,795,976 (6.57×; parse
    // 9,787,324, `LayerStyleEdit::scratch_bytes` the larger), and that
    // layer's re-edit at E 2,932,760 against 9,796,740 (3.34×).
    let mut parse = 0;
    if text.len() > REQUEST_LIMIT {
        if !is_style_request(text) {
            return Err("request exceeds the supported byte limit".into());
        }
        parse = PARSE_BYTES + 4 * text.len() as u64;
        admit(doc.retained_bytes(), parse, other, limit)?;
        job.reserve(parse)?;
    }
    // A Bitmap document is Gray underneath (`bitmap_document`), so its gate
    // goes first.
    let bitmap = if doc.is_bitmap() {
        Some(crate::bitmap_document::admit_edit(text)?)
    } else {
        None
    };
    if doc.is_gray() {
        crate::gray_document::admit_edit(text, doc.is_float())?;
    }
    if let Some(route) = bitmap {
        let edit = match route {
            crate::bitmap_document::BitmapRoute::Edit => {
                Edit::Bitmap(crate::bitmap_edit::BitmapEdit::parse(text)?)
            }
            crate::bitmap_document::BitmapRoute::Shared => Edit::parse(text)?,
        };
        return Ok((doc, edit, parse));
    }
    if let Some(policy) = doc.lab_policy() {
        crate::lab_document::admit_edit(text, policy.depth())?;
    }
    if doc.is_indexed() {
        let edit = match crate::indexed_document::admit_edit(text)? {
            crate::indexed_document::IndexedRoute::Paint => {
                Edit::IndexedPaint(crate::indexed_paint::IndexedPaintEdit::parse(text)?)
            }
            crate::indexed_document::IndexedRoute::Geometry => {
                Edit::IndexedGeometry(crate::indexed_geometry::IndexedGeometryEdit::parse(text)?)
            }
            crate::indexed_document::IndexedRoute::Selected => {
                Edit::IndexedSelected(crate::indexed_transform::IndexedSelectedEdit::parse(text)?)
            }
            crate::indexed_document::IndexedRoute::Structure
            | crate::indexed_document::IndexedRoute::Shared => Edit::parse(text)?,
        };
        return Ok((doc, edit, parse));
    }
    if doc.is_cmyk() {
        match cmyk_route(text)? {
            CmykRoute::Paint => {
                let edit = crate::cmyk_paint::CmykPaintEdit::parse(text)?;
                return Ok((doc, Edit::CmykPaint(edit), parse));
            }
            CmykRoute::LayerEdgeStroke => {
                let edit = crate::cmyk_stroke::CmykLayerEdgeStroke::parse(text)?;
                return Ok((doc, Edit::CmykLayerEdgeStroke(edit), parse));
            }
            CmykRoute::Shared => {}
        }
    }
    Ok((doc, Edit::parse(text)?, parse))
}

/// Where a CMYK document's admitted edit is parsed.
enum CmykRoute {
    /// The shared parser (`Edit::parse`), whose edits have CMYK arms.
    Shared,
    /// `cmyk_paint::CmykPaintEdit`, the four ink tools.
    Paint,
    /// `cmyk_stroke::CmykLayerEdgeStroke`, Stroke… in ink.
    LayerEdgeStroke,
}

/// A CMYK document (#1) takes only the edits of its operation table
/// (`precision_capability::CMYK_FAMILIES`): the structure edits (phase 1),
/// and since phase 2 New Layer, the four ink tools (`CmykRoute::Paint`), the
/// layer-mask edits, and the mask targets of `read_plane` and `scalar_edit`;
/// since the selection batch Apply Layer Mask, the source selections
/// (`cmyk_select`) and Stroke… (`CmykRoute::LayerEdgeStroke`); since phase 4a
/// the group, clipping, blend, style, merge and stamp edits; since phase 4b
/// the geometry edits, Perspective Crop, align/distribute and the selected
/// pixels; since phase 4c Duplicate to Document, Layer Via, Stamp Selected
/// and Merge Group; since #1's retouch batch Dodge/Burn, the retouch brushes
/// and the retouch commands (Red Eye refuses in its CMYK arm); since the
/// remaining-gaps batch the ink-plate targets of `read_plane` and
/// `scalar_edit` and `scalar_statistics` on a mask, a channel or a plate.
/// Since the Blend If batch `layer_blend_if`; also `stack_mode`, per
/// ink (`cmyk_stack_mode`). Every other op refuses by name
/// before it is parsed.
fn cmyk_route(text: &str) -> Result<CmykRoute, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("invalid high-depth edit request: {e}"))?;
    let op = value["op"].as_str().unwrap_or("");
    let target = value["target"].as_str();
    match op {
        "rename_layer" | "layer_visible" | "layer_label" | "layer_lock" | "move_layer_to"
        | "arrange_layer" | "remove_layers" | "flatten" | "add_layer" | "set_mask"
        | "remove_mask" | "mask_properties" => Ok(CmykRoute::Shared),
        "apply_mask" => Ok(CmykRoute::Shared),
        "selection" => Ok(CmykRoute::Shared),
        "paint_blend" | "bucket_fill" | "gradient" | "erase_native" => Ok(CmykRoute::Paint),
        "stroke_layer_edge" => Ok(CmykRoute::LayerEdgeStroke),
        // Phase 4a: groups, clipping masks, blend modes, Fill Opacity,
        // merges and Stamp Visible; the mode gate refuses what the mode
        // cannot hold by name (`cmyk_document::build_cmyk`).
        "group_layers" => Ok(CmykRoute::Shared),
        "ungroup_layer" => Ok(CmykRoute::Shared),
        "layer_clipped" => Ok(CmykRoute::Shared),
        "layer_open" => Ok(CmykRoute::Shared),
        "layer_blend" => Ok(CmykRoute::Shared),
        "layer_style" => Ok(CmykRoute::Shared),
        // The Blend If batch: Gray and the plates (`cmyk_document::style_refusal`).
        "layer_blend_if" => Ok(CmykRoute::Shared),
        "merge_layers" => Ok(CmykRoute::Shared),
        "merge_down" => Ok(CmykRoute::Shared),
        "merge_visible" => Ok(CmykRoute::Shared),
        "stamp_visible" => Ok(CmykRoute::Shared),
        // Phase 4b: geometry, Perspective Crop, align/distribute and the
        // selected pixels, each with a CMYK arm (`precision_geometry_cmyk`,
        // `precision_perspective_crop`, `cmyk_selected_pixels`).
        "affine" => Ok(CmykRoute::Shared),
        "perspective" => Ok(CmykRoute::Shared),
        "affine_set" => Ok(CmykRoute::Shared),
        "perspective_set" => Ok(CmykRoute::Shared),
        "resize" => Ok(CmykRoute::Shared),
        "crop" => Ok(CmykRoute::Shared),
        "canvas_size" => Ok(CmykRoute::Shared),
        "straighten" => Ok(CmykRoute::Shared),
        "rotate_canvas" => Ok(CmykRoute::Shared),
        "rotate90" => Ok(CmykRoute::Shared),
        "rotate180" => Ok(CmykRoute::Shared),
        "rotate270" => Ok(CmykRoute::Shared),
        "flip_horizontal" => Ok(CmykRoute::Shared),
        "flip_vertical" => Ok(CmykRoute::Shared),
        "perspective_crop" => Ok(CmykRoute::Shared),
        "align_set" => Ok(CmykRoute::Shared),
        "distribute_set" => Ok(CmykRoute::Shared),
        "move_selected" => Ok(CmykRoute::Shared),
        "transform_selected" => Ok(CmykRoute::Shared),
        "puppet_selected" => Ok(CmykRoute::Shared),
        "perspective_selected" => Ok(CmykRoute::Shared),
        // Phase 4c: Duplicate to (New) Document, Layer Via Copy / Cut, Stamp
        // Selected and Merge Group.
        "duplicate_to_new_document" => Ok(CmykRoute::Shared),
        "layer_via" => Ok(CmykRoute::Shared),
        "stamp_selected" => Ok(CmykRoute::Shared),
        "merge_group" => Ok(CmykRoute::Shared),
        // The channels batch: alpha channels (their planes are the RGB
        // document's, colour-free) and reading or editing one.
        "set_channel" => Ok(CmykRoute::Shared),
        "remove_channel" => Ok(CmykRoute::Shared),
        "rename_channel" => Ok(CmykRoute::Shared),
        "duplicate_channel" => Ok(CmykRoute::Shared),
        "channel_properties" => Ok(CmykRoute::Shared),
        "import_channel" => Ok(CmykRoute::Shared),
        "read_plane" if matches!(target, Some("mask" | "effective_mask" | "channel")) => {
            Ok(CmykRoute::Shared)
        }
        "scalar_edit" if matches!(target, Some("mask" | "channel")) => Ok(CmykRoute::Shared),
        // The remaining-gaps batch: one ink plate as a scalar target
        // (`cmyk_plate_scalar`), and the scalar histogram of a mask, an alpha
        // channel or a plate.
        "read_plane" | "scalar_edit"
            if matches!(target, Some("cyan" | "magenta" | "yellow" | "black")) =>
        {
            Ok(CmykRoute::Shared)
        }
        "scalar_statistics"
            if matches!(
                target,
                Some("mask" | "channel" | "cyan" | "magenta" | "yellow" | "black")
            ) =>
        {
            Ok(CmykRoute::Shared)
        }
        // #1: the retouch tools on ink (`cmyk_retouch_brush`,
        // `cmyk_retouch`), each with a CMYK arm at its entry point.
        "dodge_burn" => Ok(CmykRoute::Shared),
        "retouch_brush" => Ok(CmykRoute::Shared),
        "retouch_clone" => Ok(CmykRoute::Shared),
        "retouch_heal" => Ok(CmykRoute::Shared),
        "spot_heal" => Ok(CmykRoute::Shared),
        "content_aware_fill" => Ok(CmykRoute::Shared),
        "red_eye" => Ok(CmykRoute::Shared),
        // The styles batch: the global light (`cmyk_style`).
        "global_light" => Ok(CmykRoute::Shared),
        // Stack Modes per ink (`cmyk_stack_mode`).
        "stack_mode" => Ok(CmykRoute::Shared),
        "read_plane" | "scalar_edit" | "scalar_statistics" => {
            Err(crate::cmyk_document::refused_sentence(&format!(
                "The \"{op}\" edit of anything but a layer mask, an alpha channel or an ink plate"
            )))
        }
        op => Err(crate::cmyk_document::refused_sentence(&format!(
            "The \"{op}\" edit"
        ))),
    }
}

/// `plan` with its scratch raised to `parse`, the long request's own parse
/// charge from `request` (0 for a request within `REQUEST_LIMIT`).
fn covering_parse(mut plan: Value, parse: u64) -> Result<Value, String> {
    let scratch = number(&plan, "scratch_bytes")?;
    plan["scratch_bytes"] = json!(scratch.max(parse));
    Ok(plan)
}

fn admit_plan(plan: &Value, limit: u64, other: u64) -> Result<(), String> {
    admit(
        number(plan, "retained_bytes")?,
        number(plan, "scratch_bytes")?,
        other,
        limit,
    )
}
fn number(value: &Value, key: &str) -> Result<u64, String> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("invalid high-depth plan field {key}"))
}

/// Plan a strict bounded typed request, admitting before parameter allocation
/// and before the host allocates input/read output. Free JSON with dr_string_free.
/// # Safety
/// document NULL/live; request NULL/terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_edit_plan(
    document: *const PrecisionDocument,
    text: *const c_char,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning high-depth edit",
            ptr::null_mut(),
            || {
                let (doc, edit, parse) = request(document, text, limit, other, JobContext::NONE)?;
                let plan = covering_parse(edit.plan(doc)?, parse)?;
                admit_plan(&plan, limit, other)?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Apply a typed request; exact no-op is NULL/no error. The sample length is
/// recomputed from validated document/request extents before borrowing pixels.
/// # Safety
/// document/control NULL/live; request NULL/terminated UTF-8; samples NULL or
/// readable for length u16s; err_out NULL/writable. Owners outlive every call.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_edit(
    document: *const PrecisionDocument,
    text: *const c_char,
    samples: *const u16,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying high-depth edit",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (doc, edit, parse) = request(document, text, limit, other, job)?;
                let plan = covering_parse(edit.plan(doc)?, parse)?;
                admit_plan(&plan, limit, other)?;
                let expected = usize::try_from(number(&plan, "input_samples")?)
                    .map_err(|_| "high-depth sample count overflow")?;
                if length != expected || (expected > 0 && samples.is_null()) {
                    return Err(
                        "high-depth input sample length does not match the core plan".into(),
                    );
                }
                let samples = if expected == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(samples, expected)
                };
                match edit {
                    Edit::Geometry(edit) => doc.geometry_with_context(&edit, limit, other, job),
                    Edit::PerspectiveCrop(edit) => doc.perspective_crop(&edit, limit, other, job),
                    Edit::Align(edit) => doc.align_with_context(&edit, limit, other, job),
                    Edit::Mask(edit) => {
                        doc.mask_edit_with_context(&edit, samples, limit, other, job)
                    }
                    Edit::Channel(edit) => {
                        doc.channel_edit_with_context(&edit, samples, limit, other, job)
                    }
                    Edit::Paint(edit) => doc.paint_with_context(&edit, samples, limit, other, job),
                    Edit::NativePaint(edit) => edit.apply(doc, samples, limit, other, job),
                    Edit::CmykPaint(edit) => edit.apply(doc, samples, limit, other, job),
                    Edit::IndexedPaint(edit) => edit.apply(doc, samples, limit, other, job),
                    Edit::IndexedGeometry(edit) => edit.apply(doc, limit, other, job),
                    Edit::IndexedSelected(edit) => edit.apply(doc, samples, limit, other, job),
                    Edit::Bitmap(edit) => edit.apply(doc, samples, limit, other, job),
                    Edit::RetouchBrush(edit) => edit.apply(doc, samples, limit, other, job),
                    Edit::LayerEdgeStroke(edit) => edit.apply(doc, samples, limit, other, job),
                    Edit::CmykLayerEdgeStroke(edit) => edit.apply(doc, samples, limit, other, job),
                    Edit::Retouch(edit) => {
                        doc.retouch_with_context(&edit, samples, limit, other, job)
                    }
                    Edit::Structure(edit) => edit.apply(doc, limit, other, job),
                    Edit::Comp(edit) => edit.apply(doc, limit, other, job),
                    Edit::GlobalLight(edit) => edit.apply(doc, limit, other, job),
                    Edit::LayerBlend(edit) => edit.apply(doc, limit, other, job),
                    Edit::BlendIf(edit) => edit.apply(doc, limit, other, job),
                    Edit::Style(edit) => edit.apply(doc, limit, other, job),
                    Edit::LayerVia(edit) => edit.apply(doc, samples, limit, other, job),
                    Edit::StackMode(edit) => doc.stack_mode(&edit, limit, other, job),
                    Edit::Scalar(edit) => edit.apply(doc, samples, limit, other, job),
                    Edit::SelectedPixels(edit) => {
                        doc.selected_pixels_with_context(&edit, samples, limit, other, job)
                    }
                    Edit::ScalarStatistics(_) => {
                        Err("scalar statistics requires its read-only output API".into())
                    }
                    Edit::Selection(_) => {
                        Err("selection requires the typed selection output API".into())
                    }
                    Edit::Read(_) => Err("read_plane requires the typed read output API".into()),
                }
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Copy raw/effective typed scalar coverage. Complete staging precedes the
/// final output copy; error/cancellation leaves the caller's buffer unchanged.
/// # Safety
/// document/control NULL/live; request NULL/terminated UTF-8; out NULL or
/// writable for length u16s and nonaliasing sources; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_read_plane(
    document: *const PrecisionDocument,
    text: *const c_char,
    out: *mut u16,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "reading high-depth scalar plane",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (doc, edit, _) = request(document, text, limit, other, job)?;
                let Edit::Read(read) = edit else {
                    return Err("typed plane output requires a read_plane request".into());
                };
                let plan = read.plan(doc)?;
                admit_plan(&plan, limit, other)?;
                let expected = usize::try_from(number(&plan, "output_samples")?)
                    .map_err(|_| "high-depth scalar count overflow")?;
                if out.is_null() || length != expected {
                    return Err("high-depth output length does not match the core plan".into());
                }
                let samples = read.read_with_context(doc, limit, other, job)?;
                if let Some(control) = control {
                    control.checkpoint()?;
                }
                ptr::copy_nonoverlapping(samples.as_ptr(), out, expected);
                Ok(())
            },
            |()| true,
        )
    }
}

/// Compute canvas u16 selection from a captured typed source. No input coverage
/// is required for source selectors; `similar` reads the canvas-sized current
/// selection as its input. Complete staging precedes output copy.
/// # Safety
/// document/control NULL/live; request NULL/terminated UTF-8; input NULL or
/// readable for its planned count; out NULL or writable for the planned canvas
/// count and nonaliasing; err_out NULL/writable. Owners outlive every call.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_select(
    document: *const PrecisionDocument,
    text: *const c_char,
    samples: *const u16,
    length: usize,
    out: *mut u16,
    out_length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "creating high-depth selection",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (doc, edit, _) = request(document, text, limit, other, job)?;
                let Edit::Selection(selection) = edit else {
                    return Err("typed selection output requires a selection request".into());
                };
                let plan = selection.plan(doc)?;
                admit_plan(&plan, limit, other)?;
                let expected = number(&plan, "input_samples")? as usize;
                let count = number(&plan, "output_samples")? as usize;
                if length != expected
                    || (expected > 0 && samples.is_null())
                    || out.is_null()
                    || out_length != count
                {
                    return Err("typed selection buffer length differs from the core plan".into());
                }
                let input = if expected == 0 || samples.is_null() {
                    &[]
                } else {
                    std::slice::from_raw_parts(samples, expected)
                };
                let output = selection.compute(doc, input, limit, other, job)?;
                job.check()?;
                ptr::copy_nonoverlapping(output.as_ptr(), out, count);
                Ok(())
            },
            |()| true,
        )
    }
}

/// Derive scalar Auto Levels from raw mask/channel/RGBA codes and the same
/// optional u16 selection as scalar_edit. JSON has black/white/gamma/identity;
/// apply those values with levels_channels to publish without reanalysis.
/// # Safety
/// document/control NULL/live; request NULL/terminated UTF-8; samples NULL or
/// readable for its core-planned count; err_out NULL/writable. Owners outlive call.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_scalar_auto_levels(
    document: *const PrecisionDocument,
    text: *const c_char,
    samples: *const u16,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "deriving native scalar auto levels",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (doc, edit, _) = request(document, text, limit, other, job)?;
                let Edit::Scalar(edit) = edit else {
                    return Err("scalar Auto output requires a scalar_edit request".into());
                };
                let plan = edit.plan(doc)?;
                admit_plan(&plan, limit, other)?;
                let expected = number(&plan, "input_samples")? as usize;
                if length != expected || (expected > 0 && samples.is_null()) {
                    return Err("scalar Auto input count differs from plan".into());
                }
                let samples = if expected == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(samples, expected)
                };
                CString::new(
                    edit.auto_levels(doc, samples, limit, other, job)?
                        .to_string(),
                )
                .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Exact raw native scalar statistics. Plan through dr_precision_doc_edit_plan;
/// request op scalar_statistics, target/index, selection and positive stride.
/// RGB rows repeat the scalar with virtual opaque alpha. Owned JSON uses
/// dr_string_free; only plot bins use stride, all sums/endpoints stay complete.
/// # Safety
/// document/control NULL/live; request NULL/terminated UTF-8; samples NULL with
/// zero length or readable/aligned for length u16; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_scalar_statistics(
    document: *const PrecisionDocument,
    text: *const c_char,
    samples: *const u16,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading native scalar statistics",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (doc, edit, _) = request(document, text, limit, other, job)?;
                let Edit::ScalarStatistics(read) = edit else {
                    return Err(
                        "scalar statistics output requires a scalar_statistics request".into(),
                    );
                };
                let plan = read.plan(doc)?;
                admit_plan(&plan, limit, other)?;
                let expected = number(&plan, "input_samples")? as usize;
                if length != expected || (expected > 0 && samples.is_null()) {
                    return Err("scalar statistics input count differs from canvas coverage".into());
                }
                let input = if expected == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(samples, expected)
                };
                CString::new(read.evaluate(doc, input, limit, other, job)?.to_string())
                    .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Preflight additional canvas-sized native channels before generating coverage.
/// Includes native serialized pixel/plane capacity; does not allocate pixels.
/// # Safety
/// document NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_channel_capacity(
    document: *const PrecisionDocument,
    additional: usize,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "planning native channel capacity",
            false,
            || {
                let doc = required_handle(document)?;
                let count = doc
                    .channels()
                    .len()
                    .checked_add(additional)
                    .ok_or("native channel count overflow")?;
                if count > crate::doc_channel::MAX_CHANNELS {
                    return Err("additional channels exceed the native channel count limit".into());
                }
                let (rasters, masks) = doc.native_dimensions();
                doc.native_fits_geometry(doc.dimensions(), &rasters, &masks, count)
            },
            |()| true,
        )
    }
}
