//! VECTOR MASKS, the depth-neutral half — what `doc_vector_mask` (8
//! bits, whose module doc is the specification) and its typed twin
//! `precision_vector_mask` share: the host's view of a vector mask and its
//! JSON, the seed a geometry op or a depth conversion carries, the bake
//! rectangle and the path factor's rule, the document anchor-cap refusal, and
//! the PATH-table agreement check both `.rz` readers run.

use serde_json::{json, Value};

use crate::doc_mask::MaskProperties;
use crate::doc_path::{anchor_cap_sentence, MAX_DOCUMENT_PATH_ANCHORS};
use crate::path_geometry::PathShape;
use crate::path_native::TableMask;

/// What a host reads of a layer's vector mask.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorMaskView {
    pub shape: PathShape,
    pub enabled: bool,
    pub linked: bool,
    /// Whether a raster mask is kept beside the shape.
    pub has_raster: bool,
    /// That raster mask's own enabled flag; false without one.
    pub raster_enabled: bool,
    /// The raster part's own placement and properties. The layer's ordinary
    /// mask getters describe the BAKED mask, so a host that edits a mask
    /// property by read-modify-write must start from these instead, or it
    /// would write the baked mask's offset, density and feather back over
    /// the raster part's.
    pub raster: Option<RasterPartView>,
}

/// A raster part as a host reads it: `dr_doc_layer_mask_properties`' fields
/// for the mask kept beside a shape.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterPartView {
    pub offset: (i32, i32),
    pub dimensions: (u32, u32),
    pub linked: bool,
    pub density: f32,
    pub feather: f32,
    pub outside_coverage: u8,
}

impl RasterPartView {
    pub(crate) fn of(properties: &MaskProperties, dimensions: (u32, u32)) -> Self {
        RasterPartView {
            offset: properties.offset,
            dimensions,
            linked: properties.linked,
            density: properties.density,
            feather: properties.feather,
            outside_coverage: properties.outside_coverage,
        }
    }
}

impl VectorMaskView {
    /// The FFI's vector-mask JSON: the shape's own keys beside the flags and
    /// `raster` — null, or the raster part's mask-properties object.
    pub(crate) fn json(&self) -> String {
        let mut value = self.shape.to_value();
        if let Value::Object(object) = &mut value {
            object.insert("enabled".into(), json!(self.enabled));
            object.insert("linked".into(), json!(self.linked));
            object.insert("has_raster".into(), json!(self.has_raster));
            object.insert("raster_enabled".into(), json!(self.raster_enabled));
            object.insert(
                "raster".into(),
                self.raster.as_ref().map_or(Value::Null, |raster| {
                    json!({
                        "offset_x": raster.offset.0, "offset_y": raster.offset.1,
                        "width": raster.dimensions.0, "height": raster.dimensions.1,
                        "linked": raster.linked, "density": raster.density,
                        "feather": raster.feather,
                        "outside_coverage": raster.outside_coverage,
                    })
                }),
            );
        }
        value.to_string()
    }
}

/// A raster-less vector mask without its plane: what crosses a geometry op
/// and a depth conversion, where the plane is rebuilt from the shape.
#[derive(Clone)]
pub(crate) struct VectorMaskSeed {
    pub(crate) layer_id: u64,
    pub(crate) shape: PathShape,
    pub(crate) enabled: bool,
    pub(crate) linked: bool,
}

/// Whether the shape contributes coverage: an enabled vector mask with at
/// least one subpath. An empty shape is Reveal All.
pub(crate) fn path_on(shape: &PathShape, enabled: bool) -> bool {
    enabled && !shape.subpaths.is_empty()
}

/// `shape` moved by a whole-pixel delta; the same shape for none.
pub(crate) fn shifted(shape: &PathShape, dx: i64, dy: i64) -> PathShape {
    if dx == 0 && dy == 0 {
        return shape.clone();
    }
    let (fx, fy) = (dx as f64, dy as f64);
    shape.mapped(|x, y| (x + fx, y + fy))
}

/// The rectangle a path plane is baked over, `(x, y, width, height)`: the
/// shape's pixel box (its control hull, which contains the curve, grown to
/// whole pixels) inside `within`. A shape that misses `within` altogether
/// answers one pixel at `within`'s origin, which the rasterizer leaves at 0:
/// a mask that hides everything still needs a plane.
pub(crate) fn bake_rect(shape: &PathShape, within: (i64, i64, i64, i64)) -> (i32, i32, u32, u32) {
    let (wx, wy, ww, wh) = within;
    let one = (wx as i32, wy as i32, 1, 1);
    let Some([min_x, min_y, max_x, max_y]) = shape.bounds() else {
        return one;
    };
    let x0 = (min_x.floor() as i64).max(wx);
    let y0 = (min_y.floor() as i64).max(wy);
    let x1 = (max_x.ceil() as i64).min(wx + ww);
    let y1 = (max_y.ceil() as i64).min(wy + wh);
    if x1 <= x0 || y1 <= y0 {
        return one;
    }
    (x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32)
}

/// The bounding rectangle of two `(x, y, width, height)` rectangles.
pub(crate) fn union_rect(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> (i64, i64, i64, i64) {
    let x0 = a.0.min(b.0);
    let y0 = a.1.min(b.1);
    let x1 = (a.0 + a.2).max(b.0 + b.2);
    let y1 = (a.1 + a.3).max(b.1 + b.3);
    (x0, y0, x1 - x0, y1 - y0)
}

/// The refusal both depths give a shape that would take the document past
/// `MAX_DOCUMENT_PATH_ANCHORS`: `others` is every anchor the document would
/// still hold beside the new shape.
pub(crate) fn anchor_refusal(others: usize, shape: &PathShape) -> Option<String> {
    (others.saturating_add(shape.anchor_count()) > MAX_DOCUMENT_PATH_ANCHORS)
        .then(anchor_cap_sentence)
}

/// The refusal both depths' readers give a PATH-table record that does not
/// agree with its layer's stored mask.
pub(crate) const TABLE_MISMATCH: &str = "invalid native path table vector mask section";

/// Whether a PATH-table record agrees with its layer's stored mask, given
/// that mask's dimensions (`None` for a layer without one) — the module
/// doc's slot rule read backwards, checked by both readers before any plane
/// is decoded or copied: a raster part or a non-empty shape leaves a mask
/// and nothing else does; the raster part is the stored mask exactly when
/// the shape contributes nothing; and a raster part that is the stored mask
/// has properties that mask's dimensions admit.
pub(crate) fn table_mask_agrees(mask: &TableMask<'_>, slot: Option<(u32, u32)>) -> bool {
    let expects_slot = mask.raster.is_some() || !mask.shape.subpaths.is_empty();
    if slot.is_some() != expects_slot {
        return false;
    }
    let on = path_on(&mask.shape, mask.enabled);
    mask.raster.as_ref().is_none_or(|raster| {
        raster.plane.is_none() != on
            && (raster.plane.is_some()
                || slot.is_some_and(|dimensions| raster.properties().valid(dimensions)))
    })
}
