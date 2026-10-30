//! The geometry plans a CMYK document's descriptor reports (#1 phase 4b):
//! `get_precision`'s CMYK `plans` and `scripts/cmyk-admission`'s gated
//! figures. Each is the core plan's `scratch_bytes` of one FIXED request,
//! written beside its key, on the top CMYK raster (`None` where the request
//! refuses: no raster record, a lock, a degenerate canvas). The admission
//! drive sends the same requests.
//!
//! - `cmyk_resize_scratch_bytes`: Image Size to ⌈3w/2⌉×⌈3h/2⌉, bicubic.
//! - `cmyk_affine_scratch_bytes`: the top raster's pixels turned 17° about
//!   the canvas centre, bilinear.
//! - `cmyk_perspective_crop_scratch_bytes`: the quad inset 10 % with a 5 %
//!   keystone (the top edge pulled in by 5 % of the width each side) onto
//!   w×h, bilinear.
//! - `cmyk_warp_scratch_bytes`: a 4×4 bicubic warp of the top raster's
//!   pixels with node (1, 1) moved (60, 40).
//! - `cmyk_liquify_scratch_bytes`: one Forward Warp stroke, size 500, from
//!   the canvas centre 300 px to the right.
//! - `cmyk_transform_selected_scratch_bytes`: `transform_selected` 7° about
//!   the canvas centre, bilinear, `bounds` the canvas.
//! - `cmyk_move_plate_scratch_bytes` / `cmyk_transform_plate_scratch_bytes`
//!   (the channels batch): `move_selected` dx 37 dy 23 and the same 7°
//!   `transform_selected`, `"target":"cyan"` (`cmyk_selected_plate`),
//!   `bounds` the canvas.

use serde_json::{json, Value};

use crate::precision_document::PrecisionDocument;

/// `[a, b, c, d, tx, ty]` turning `degrees` about `(cx, cy)`
/// (`x' = a·x + c·y + tx`, `y' = b·x + d·y + ty`).
fn turn(degrees: f64, (cx, cy): (f64, f64)) -> [f64; 6] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    [
        cos,
        sin,
        -sin,
        cos,
        cx - cos * cx + sin * cy,
        cy - sin * cx - cos * cy,
    ]
}

impl PrecisionDocument {
    /// The six geometry plans (module doc).
    pub(crate) fn cmyk_geometry_plans(&self) -> Value {
        let top = (0..self.layer_count())
            .rev()
            .find(|&i| self.layer(i).and_then(|l| l.cmyk_source()).is_some());
        let (w, h) = self.dimensions();
        let (fw, fh) = (f64::from(w), f64::from(h));
        let centre = (fw / 2.0, fh / 2.0);
        let geometry = |request: String| {
            crate::precision_geometry::GeometryEdit::parse(&request)
                .and_then(|edit| self.geometry_plan(&edit))
                .ok()
                .map(|plan| plan.scratch_bytes)
        };
        let resize = geometry(format!(
            r#"{{"op":"resize","width":{},"height":{},"filter":"bicubic"}}"#,
            (3 * u64::from(w)).div_ceil(2),
            (3 * u64::from(h)).div_ceil(2)
        ));
        let affine = top.and_then(|i| {
            geometry(
                json!({"op":"affine","layer":i,"target":"pixels",
                    "matrix":turn(17.0, centre),"filter":"bilinear"})
                .to_string(),
            )
        });
        let quad = [
            0.15 * fw,
            0.10 * fh,
            0.85 * fw,
            0.10 * fh,
            0.90 * fw,
            0.90 * fh,
            0.10 * fw,
            0.90 * fh,
        ];
        let perspective_crop = crate::perspective_crop::PerspectiveCropRequest::parse(
            &json!({"op":"perspective_crop","quad":quad,"width":w,"height":h,"filter":"bilinear"})
                .to_string(),
        )
        .and_then(|request| self.perspective_crop_plan(&request))
        .ok()
        .and_then(|plan| plan["scratch_bytes"].as_u64());
        let warp = top.and_then(|i| {
            let mut displacements = vec![[0.0, 0.0]; 16];
            displacements[5] = [60.0, 40.0];
            crate::warp::WarpEdit::parse(
                &json!({"op":"set","layer":i,"target":"pixels","columns":4,"rows":4,
                    "displacements":displacements,"filter":"bicubic"})
                .to_string(),
            )
            .and_then(|edit| self.warp_plan(&edit))
            .ok()
            .map(|plan| plan.scratch_bytes)
        });
        let liquify = top.and_then(|i| {
            let params = json!({"strokes":[{"tool":"forward_warp","size":500,
                "points":[[centre.0, centre.1],[centre.0 + 300.0, centre.1]]}]});
            let strokes = params
                .as_object()
                .and_then(|p| crate::liquify::parse_strokes(p).ok())?;
            self.cmyk_liquify_plan(i, &strokes)
                .ok()
                .and_then(|plan| plan["scratch_bytes"].as_u64())
        });
        let transform_selected = top.and_then(|i| {
            crate::selected_pixels::SelectedPixelsEdit::parse(
                &json!({"op":"transform_selected","layer":i,"matrix":turn(7.0, centre),
                    "filter":"bilinear","bounds":[0, 0, w, h]})
                .to_string(),
            )
            .and_then(|edit| self.selected_pixels_plan(&edit))
            .ok()
            .map(|plan| plan.scratch_bytes)
        });
        let plate = |request: Value| {
            top.and_then(|i| {
                let mut request = request;
                request["layer"] = json!(i);
                crate::selected_pixels::SelectedPixelsEdit::parse(&request.to_string())
                    .and_then(|edit| self.selected_pixels_plan(&edit))
                    .ok()
                    .map(|plan| plan.scratch_bytes)
            })
        };
        let move_plate = plate(json!({"op":"move_selected","dx":37,"dy":23,"target":"cyan",
            "bounds":[0, 0, w, h]}));
        let transform_plate = plate(json!({"op":"transform_selected","matrix":turn(7.0, centre),
            "filter":"bilinear","target":"cyan","bounds":[0, 0, w, h]}));
        json!({
            "cmyk_move_plate_scratch_bytes": move_plate,
            "cmyk_transform_plate_scratch_bytes": transform_plate,
            "cmyk_resize_scratch_bytes": resize,
            "cmyk_affine_scratch_bytes": affine,
            "cmyk_perspective_crop_scratch_bytes": perspective_crop,
            "cmyk_warp_scratch_bytes": warp,
            "cmyk_liquify_scratch_bytes": liquify,
            "cmyk_transform_selected_scratch_bytes": transform_selected,
        })
    }
}
