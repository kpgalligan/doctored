//! The Image-menu geometry on an Indexed Color document (#1 Indexed phase
//! 2c, `designs/indexed-mode.md`): Image Size, Canvas Size, Trim and
//! Crop, Image Rotation, flip and Perspective Crop, and since phase 2d Free
//! Transform and Transform on the one layer (`affine`, `perspective`,
//! `affine_set`, `perspective_set`), through the existing kernels
//! (`geometry_with_context`, `perspective_crop`). An adapter, not a kernel:
//! it names the document types.
//!
//! Two paths, decided by the request ([`IndexedGeometryEdit::exact`]):
//!
//! - **Exact** (a move or a Nearest Neighbor resample: `crop`, `canvas_size`,
//!   the quarter turns and flips, `resize`, `rotate_canvas`, `straighten`,
//!   `perspective_crop`, `perspective` and `perspective_set` with filter
//!   Nearest, and `affine` and `affine_set` with filter Nearest or a matrix
//!   `[1, 0, 0, 1, tx, ty]` of integer tx, ty, run Nearest). Every output
//!   pixel is one input pixel or new area, so the index plane itself moves
//!   ([`carried`]): the plane is encoded as a CARRIER layer, index i as
//!   `[i·257, i·257, i·257, 65535]` (no profile arithmetic touches an
//!   alpha-65535 grey code under a move or a nearest pick) and, when the new
//!   area is t, t as `[0, 0, 0, 0]`, so a float of t lands nothing, as on the
//!   RGB storage; the same request runs on the carrier document, and the
//!   canvas window is read back, alpha 65535 as index `R/257`. Duplicate
//!   palette entries keep their own index, which no colour lookup could.
//!   New area (alpha 0) takes `indexed_edge::clear_index`: the transparent
//!   index t, else (since phase 3a) the entry nearest the background colour,
//!   as a Background layer extends; any other alpha is an error.
//!   A `straighten` carrying `background` (the Crop tool
//!   and MCP `crop` past the canvas) runs on the carrier without it, and
//!   when the RGB route would pad (`doc_crop_extend`'s Background rule: an
//!   opaque record 0 with Delete Cropped Pixels on, `background_pad_bytes`
//!   above 0) its new area takes E, the entry nearest the background colour
//!   as `doc_crop_extend` prepares it (never t, so t stays opaque on that
//!   carrier). A Nearest `straighten` with `background` is exact only at the
//!   identity matrix, the zero-angle crop.
//! - **Resample** (any other filter): the request runs on the RGB storage,
//!   then `indexed_paint::republish` re-indexes the canvas window with each
//!   pixel's own colour (`EdgeInk::Own`): alpha below 128 takes
//!   `clear_index`, the rest its nearest entry. The whole-canvas
//!   geometry has no backdrop (every pixel changed); a layer transform keeps
//!   the canvas, so it is compared with the old plane and an unchanged pixel
//!   keeps its index.
//!
//! A layer transform of the mask target refuses for good with
//! `transform_mask`'s sentence (phase 3a): the Index layer holds no mask.
//!
//! Since phase 3b the background colour is the request's: a Straighten's own
//! `background`, else `indexed_paint::BACKGROUND_KEY` (the host's swatch),
//! else white, each prepared by `indexed_paint::background_codes`; the new
//! area and a resample's clear pixels take its entry when there is no t (a
//! Straighten's own colour with no pad included), and the padded crop's new
//! area takes `indexed_palette::nearest_entry` of it.
//!
//! The plan is the inner plan on the RGB storage plus [`transform_scratch`]:
//! (old + new canvas) × [`GEOMETRY_BYTES_PER_PIXEL`], the nearest-colour
//! memo and bookkeeping.

use serde_json::{json, Value};

use crate::indexed_document::{refused_sentence, IndexedPolicy};
use crate::indexed_edge::{clear_index, EdgeInk};
use crate::indexed_paint::{
    background_codes, canvas_row, rebuild, republish, split_background, NOT_INDEXED,
};
use crate::indexed_palette::{nearest_entry, Rgb, MEMO_BYTES};
use crate::job_control::JobContext;
use crate::perspective_crop::PerspectiveCropRequest;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_geometry::{CropBackground, GeometryEdit, GeometryTarget};
use crate::precision_image::{admit, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_sampling::GeometryFilter;
use crate::tile_store::TilePlane;

/// The adapter's working memory per pixel of the old canvas and of the new
/// one, beside the inner op's own: the carrier layer or the row of codes
/// (8 of the old), the carrier the inner op moves, the new plane (1), the
/// re-rendered RGBA16 layer (8 of the new) and the host's display
/// preparation of the result, with slack (`scripts/indexed-admission/
/// measure.py`). Phase 2c declared 10, which held where the inner op plans
/// its own output (`rotate90-indexed` 1.91×, `rotate-indexed` 1.45×); phase
/// 2d's whole-pixel layer translation plans almost nothing inside, and at 10
/// `transform-nearest-indexed` (24 MP, Release, 3 runs) missed: plan
/// 519,848,838 against max E 794,313,424 (0.654×). 20 would clear the
/// 1.25× need of 992,891,780 by 0.7 %, so 21.
pub(crate) const GEOMETRY_BYTES_PER_PIXEL: u64 = 21;

/// The identity matrix a zero-angle `straighten` carries.
const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// Whether `matrix` is `[1, 0, 0, 1, tx, ty]` with integer tx and ty: a
/// pure whole-pixel move, exact under any filter (module doc).
pub(crate) fn integer_translation(matrix: &[f64; 6]) -> bool {
    matrix[..4] == IDENTITY[..4]
        && matrix[4..]
            .iter()
            .all(|v| v.is_finite() && v.fract() == 0.0)
}

/// The adapter's scratch beside an inner plan of `inner` bytes, over the
/// old canvas and a `new` one (module doc): the one home of 2c's formula,
/// shared by every Indexed transform.
pub(crate) fn transform_scratch(doc: &PrecisionDocument, inner: u64, new: (u32, u32)) -> u64 {
    let pixels = (u64::from(doc.width) * u64::from(doc.height))
        .saturating_add(u64::from(new.0) * u64::from(new.1));
    inner
        .saturating_add(pixels.saturating_mul(GEOMETRY_BYTES_PER_PIXEL))
        .saturating_add(MEMO_BYTES)
        .saturating_add(BOOKKEEPING_BYTES)
}

/// One geometry edit on an Indexed document (module doc).
pub(crate) struct IndexedGeometryEdit {
    op: IndexedGeometryOp,
    /// The request's `indexed_paint::BACKGROUND_KEY` (phase 3b).
    background: Option<CropBackground>,
}

/// The existing kernel an [`IndexedGeometryEdit`] runs.
enum IndexedGeometryOp {
    Canvas(GeometryEdit),
    PerspectiveCrop(PerspectiveCropRequest),
}

impl IndexedGeometryEdit {
    /// A whole-canvas geometry or `perspective_crop` request; every other op
    /// refuses with the mode's sentence (`indexed_document::admit_edit`
    /// asks first).
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let (stripped, background) = split_background(text)?;
        let text = stripped.as_str();
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("invalid high-depth edit request: {e}"))?;
        let op = match value["op"].as_str().unwrap_or("edit") {
            "perspective_crop" => {
                IndexedGeometryOp::PerspectiveCrop(PerspectiveCropRequest::parse(text)?)
            }
            "resize" | "crop" | "canvas_size" | "straighten" | "rotate_canvas" | "rotate90"
            | "rotate180" | "rotate270" | "flip_horizontal" | "flip_vertical" | "affine"
            | "perspective" | "affine_set" | "perspective_set" => {
                let edit = GeometryEdit::parse(text)?;
                if let GeometryEdit::Affine {
                    target: GeometryTarget::Mask,
                    ..
                }
                | GeometryEdit::Perspective {
                    target: GeometryTarget::Mask,
                    ..
                } = edit
                {
                    // The Index layer holds no mask.
                    return Err(refused_sentence("transform_mask"));
                }
                IndexedGeometryOp::Canvas(edit)
            }
            op => return Err(refused_sentence(op)),
        };
        Ok(Self { op, background })
    }

    /// The background colour at 8-bit codes (module doc): a Straighten's own
    /// `background`, else the request's key, else white.
    fn background_color(&self, rgb: &PrecisionDocument) -> Result<Rgb, String> {
        let own = match &self.op {
            IndexedGeometryOp::Canvas(GeometryEdit::Straighten { background, .. }) => {
                background.as_ref()
            }
            _ => None,
        };
        background_codes(rgb, own.or(self.background.as_ref()))
    }

    /// Whether the edit only moves pixels or picks them Nearest Neighbor
    /// (module doc).
    fn exact(&self) -> bool {
        let nearest = |filter: &GeometryFilter| matches!(filter, GeometryFilter::Nearest);
        match &self.op {
            IndexedGeometryOp::Canvas(edit) => match edit {
                GeometryEdit::Crop { .. }
                | GeometryEdit::CanvasSize { .. }
                | GeometryEdit::Rotate90
                | GeometryEdit::Rotate180
                | GeometryEdit::Rotate270
                | GeometryEdit::FlipHorizontal
                | GeometryEdit::FlipVertical => true,
                GeometryEdit::Resize { filter, .. }
                | GeometryEdit::RotateCanvas { filter, .. }
                | GeometryEdit::Perspective { filter, .. }
                | GeometryEdit::PerspectiveSet { filter, .. } => nearest(filter),
                GeometryEdit::Affine { filter, matrix, .. }
                | GeometryEdit::AffineSet { filter, matrix, .. } => {
                    nearest(filter) || integer_translation(matrix)
                }
                GeometryEdit::Straighten {
                    filter,
                    matrix,
                    background,
                    ..
                } => nearest(filter) && (background.is_none() || *matrix == IDENTITY),
            },
            IndexedGeometryOp::PerspectiveCrop(request) => nearest(&request.filter),
        }
    }

    /// Whether the edit keeps the canvas and so is re-indexed against the old
    /// plane (a layer transform, module doc).
    fn keeps_canvas(&self) -> bool {
        matches!(
            &self.op,
            IndexedGeometryOp::Canvas(
                GeometryEdit::Affine { .. }
                    | GeometryEdit::Perspective { .. }
                    | GeometryEdit::AffineSet { .. }
                    | GeometryEdit::PerspectiveSet { .. }
            )
        )
    }

    /// The inner plan on `rgb` and the new canvas.
    fn inner_plan(&self, rgb: &PrecisionDocument) -> Result<(u64, u64, u64, (u32, u32)), String> {
        match &self.op {
            IndexedGeometryOp::Canvas(edit) => {
                let p = rgb.geometry_plan(edit)?;
                Ok((
                    p.retained_bytes,
                    p.scratch_bytes,
                    p.work_units,
                    (p.width, p.height),
                ))
            }
            IndexedGeometryOp::PerspectiveCrop(request) => {
                let p = rgb.perspective_crop_plan(request)?;
                let number = |key: &str| {
                    p[key]
                        .as_u64()
                        .ok_or_else(|| format!("perspective crop plan has no {key}"))
                };
                Ok((
                    number("retained_bytes")?,
                    number("scratch_bytes")?,
                    0,
                    (request.width, request.height),
                ))
            }
        }
    }

    /// The inner plan on the RGB storage plus the adapter's bytes (module
    /// doc), in `Edit::Geometry`'s shape.
    pub(crate) fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        doc.indexed_policy().ok_or(NOT_INDEXED)?;
        let (retained, scratch, work, new) = self.inner_plan(&doc.without_indexed())?;
        let scratch = transform_scratch(doc, scratch, new);
        Ok(
            json!({"retained_bytes":retained,"scratch_bytes":scratch,"input_samples":0,"work_units":work}),
        )
    }

    /// The edit on `doc` (module doc); `Ok(None)` when nothing moves.
    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let policy = doc.indexed_policy().ok_or(NOT_INDEXED)?;
        let scratch = self.plan(doc)?["scratch_bytes"]
            .as_u64()
            .ok_or("indexed geometry plan has no scratch_bytes")?;
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let rgb = doc.without_indexed();
        let background = self.background_color(&rgb)?;
        if !self.exact() {
            let inner = job.range(0.0, 0.7).with_reservation_floor(scratch);
            let Some(moved) = self.run(&rgb, &self.op, limit, other, inner)? else {
                return Ok(None);
            };
            let compare = self.keeps_canvas();
            return republish(
                doc,
                &moved,
                EdgeInk::Own,
                compare,
                background,
                job.range(0.7, 1.0),
            );
        }
        // The new area's index: E of the Background colour where the RGB
        // route would pad (never t), else `clear_index` (t, else E of the
        // background colour).
        let new_area = match &self.op {
            IndexedGeometryOp::Canvas(
                edit @ GeometryEdit::Straighten {
                    background: Some(_),
                    ..
                },
            ) if rgb.background_pad_bytes(edit)? > 0 => nearest_entry(
                policy.palette(),
                policy.transparent().map(usize::from),
                background,
            ),
            _ => clear_index(policy.palette(), policy.transparent(), background),
        };
        // The same request on the carrier, a Straighten's background dropped
        // and a layer transform's filter Nearest.
        let op = match &self.op {
            IndexedGeometryOp::Canvas(edit) => {
                let mut edit = edit.clone();
                match &mut edit {
                    GeometryEdit::Straighten { background, .. } => *background = None,
                    GeometryEdit::Affine { filter, .. }
                    | GeometryEdit::AffineSet { filter, .. }
                    | GeometryEdit::Perspective { filter, .. }
                    | GeometryEdit::PerspectiveSet { filter, .. } => {
                        *filter = GeometryFilter::Nearest
                    }
                    _ => {}
                }
                IndexedGeometryOp::Canvas(edit)
            }
            IndexedGeometryOp::PerspectiveCrop(request) => {
                IndexedGeometryOp::PerspectiveCrop(*request)
            }
        };
        carried(
            doc,
            new_area,
            |carrier, inner| self.run(carrier, &op, limit, other, inner),
            job.with_reservation_floor(scratch),
        )
    }

    /// `op` run on `doc` through its existing kernel.
    fn run(
        &self,
        doc: &PrecisionDocument,
        op: &IndexedGeometryOp,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        match op {
            IndexedGeometryOp::Canvas(edit) => doc.geometry_with_context(edit, limit, other, job),
            IndexedGeometryOp::PerspectiveCrop(request) => {
                doc.perspective_crop(request, limit, other, job)
            }
        }
    }
}

/// The exact path every Indexed move shares (module doc): `doc`'s plane as
/// the carrier (t cleared only when `new_area` is t), `run` on the carrier
/// document, the canvas window read back (alpha 0 as `new_area`) and the
/// document rebuilt; `Ok(None)` when `run` changes nothing or the plane and
/// canvas come back unchanged. `job` carries the caller's reservation floor.
pub(crate) fn carried(
    doc: &PrecisionDocument,
    new_area: u8,
    run: impl FnOnce(&PrecisionDocument, JobContext<'_>) -> Result<Option<PrecisionDocument>, String>,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    let policy = doc.indexed_policy().ok_or(NOT_INDEXED)?;
    let carrier = carrier(
        &doc.without_indexed(),
        policy,
        new_area,
        job.range(0.0, 0.1),
    )?;
    let Some(moved) = run(&carrier, job.range(0.1, 0.7))? else {
        return Ok(None);
    };
    let [layer] = moved.root_layers() else {
        return Err("an Indexed Color document holds one layer".into());
    };
    let Content::Raster { image, offset } = &layer.content else {
        return Err("an Indexed Color document's layer is a raster".into());
    };
    let (width, height) = moved.dimensions();
    let entries = policy.palette().len();
    let mut plane = Vec::new();
    plane
        .try_reserve_exact(width as usize * height as usize)
        .map_err(|_| "the Indexed Color plane could not be allocated")?;
    let mut row = vec![[0u16; 4]; width as usize];
    let rows = job.range(0.7, 0.8);
    for y in 0..height {
        canvas_row(image, *offset, y, &mut row)?;
        for pixel in &row {
            let index = match pixel {
                [r, g, b, 65535]
                    if r == g && g == b && r % 257 == 0 && usize::from(r / 257) < entries =>
                {
                    (r / 257) as u8
                }
                [.., 0] => new_area,
                _ => return Err("the Indexed Color index plane did not move exactly".into()),
            };
            plane.push(index);
        }
        rows.report(f64::from(y + 1) / f64::from(height), 5)?;
    }
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
    rebuild(&moved, next, job.range(0.8, 1.0)).map(Some)
}

/// `rgb` (the storage) with its one layer's content replaced by the carrier
/// of `policy`'s plane (module doc), at the origin: t as `[0, 0, 0, 0]` when
/// `new_area` is t, so a float of t lands nothing.
fn carrier(
    rgb: &PrecisionDocument,
    policy: &IndexedPolicy,
    new_area: u8,
    job: JobContext<'_>,
) -> Result<PrecisionDocument, String> {
    let [layer] = rgb.root_layers() else {
        return Err("an Indexed Color document holds one layer".into());
    };
    let (width, height) = policy.dimensions();
    let indices = policy.indices();
    let clear = policy.transparent().filter(|&t| t == new_area);
    let w = width as usize;
    let tiles = TilePlane::<[u16; 4]>::from_rows_with_job(
        width,
        height,
        |y, row| {
            let start = y as usize * w;
            for (pixel, &index) in row.iter_mut().zip(&indices[start..start + w]) {
                let code = u16::from(index) * 257;
                *pixel = if Some(index) == clear {
                    [0; 4]
                } else {
                    [code, code, code, 65535]
                };
            }
            job.every(start)
        },
        job,
    )?;
    let image = PrecisionImage::from_rgba16_tiles_unseeded(tiles, rgb.profile().clone())?;
    let mut layer = layer.clone();
    layer.content = Content::Raster {
        image,
        offset: (0, 0),
    };
    rgb.rebuilt(width, height, vec![layer], rgb.channels().to_vec())
}
