//! Typed Merge Layers and Merge Down keep the UNION EXTENT of their
//! visible members, at 16 bits and at 32-bit float, as the byte merges do —
//! so pixels a member holds outside the canvas survive the merge.
//!
//! Pixel claims are checked against an f64 source-over oracle written out
//! here (`over`), against the document's own PRE-MERGE render (a merge must
//! not change the picture on the canvas), or against the byte merge of the
//! same stack promoted ×257 — never against a golden. Every refusal is
//! checked by its SENTENCE, verbatim, in the edit and in the plan.

use crate::common;

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_layer_count, dr_doc_merging_down};
use doctored_core::ffi_group::{dr_doc_group_layers, dr_doc_merge_layers};
use doctored_core::ffi_precision::dr_precision_doc_duplicate_layer;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

use common::{layer_dims, layer_offset, layer_pixels, solid};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

// --------------------------------------------------- the independent oracle --

/// Straight source-over of ONE pixel in f64 — the W3C formula written out,
/// reading nothing from the crate.
fn over(b: [f64; 4], s: [f64; 4], opacity: f64) -> [f64; 4] {
    let sa = s[3] * opacity;
    let out_a = sa + b[3] * (1.0 - sa);
    if out_a <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (s[c] * sa + b[c] * b[3] * (1.0 - sa)) / out_a;
    }
    out[3] = out_a;
    out
}

fn unit16(p: [u16; 4]) -> [f64; 4] {
    p.map(|v| f64::from(v) / 65535.0)
}

// ------------------------------------------------------------- plumbing --

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    unsafe {
        let text = CStr::from_ptr(error).to_string_lossy().into_owned();
        dr_string_free(error);
        Some(text)
    }
}

fn edit(
    document: &PrecisionDocument,
    request: &Value,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    Ok((!result.is_null()).then(|| *unsafe { Box::from_raw(result) }))
}

fn must(document: &PrecisionDocument, request: Value) -> PrecisionDocument {
    match edit(document, &request) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn refusal(document: &PrecisionDocument, request: Value) -> String {
    match edit(document, &request) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    }
}

fn plan_result(document: &PrecisionDocument, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(document, text.as_ptr(), LIMIT, 0, &mut error) };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    assert!(!out.is_null(), "a plan with no error");
    let value = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    Ok(serde_json::from_str(&value).expect("plan JSON"))
}

fn plan_scratch(document: &PrecisionDocument, request: Value) -> u64 {
    match plan_result(document, &request) {
        Ok(plan) => plan["scratch_bytes"].as_u64().expect("scratch_bytes"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn plan_refusal(document: &PrecisionDocument, request: Value) -> String {
    match plan_result(document, &request) {
        Err(message) => message,
        Ok(_) => panic!("the plan of {request} must be refused"),
    }
}

fn rows(document: &PrecisionDocument) -> Vec<Value> {
    let value: Value = serde_json::from_str(&document.info().expect("info").to_string()).unwrap();
    value["layers"].as_array().expect("layer rows").clone()
}

/// `(offset, dimensions)` of record `index`, read through `info`.
fn placed(document: &PrecisionDocument, index: usize) -> (Value, Value) {
    let row = &rows(document)[index];
    (row["offset"].clone(), row["dimensions"].clone())
}

fn render16(document: &PrecisionDocument) -> Vec<[u16; 4]> {
    document
        .render(LIMIT, 0, None)
        .expect("render")
        .to_rgba16()
        .expect("u16 storage")
}

fn render32(document: &PrecisionDocument) -> Vec<[f32; 4]> {
    document
        .render(LIMIT, 0, None)
        .expect("render")
        .to_rgba32()
        .expect("f32 storage")
}

fn samples(document: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    document
        .layer_source(index)
        .expect("a raster record")
        .to_rgba16()
        .expect("u16 storage")
}

/// Every channel of every pixel within `tolerance` codes.
fn within16(got: &[[u16; 4]], want: &[[u16; 4]], tolerance: u16, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..4 {
            assert!(
                g[c].abs_diff(w[c]) <= tolerance,
                "{what}: pixel {i} channel {c} is {} against {} (tolerance {tolerance})",
                g[c],
                w[c]
            );
        }
    }
}

/// Every channel within a RELATIVE 1e-5 (scale at least 1).
fn within32(got: &[[f32; 4]], want: &[[f32; 4]], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..4 {
            let difference = (f64::from(g[c]) - f64::from(w[c])).abs();
            let scale = f64::from(w[c]).abs().max(1.0);
            assert!(
                difference <= 1e-5 * scale,
                "{what}: pixel {i} channel {c} is {} against {} (relative 1e-5)",
                g[c],
                w[c]
            );
        }
    }
}

fn crop(width: u32, height: u32) -> Value {
    json!({"op": "crop", "x": 0, "y": 0, "width": width, "height": height})
}

fn canvas_size(width: u32, height: u32) -> Value {
    json!({"op": "canvas_size", "width": width, "height": height, "x": 0, "y": 0})
}

fn moved(layers: &[usize], dx: i32, dy: i32) -> Value {
    json!({"op": "affine_set", "layers": layers, "matrix": [1, 0, 0, 1, dx, dy],
           "filter": "nearest"})
}

// ------------------------------------------------------------- fixtures --

fn image(width: u32, height: u32, pixel: [u16; 4]) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        width,
        height,
        vec![pixel; (width * height) as usize],
        IccProfile::srgb(),
        None,
    )
    .expect("an image")
}

/// A varying image, so a misplaced sample cannot pass as a correct one.
fn pattern(width: u32, height: u32, seed: u16) -> PrecisionImage {
    let pixels = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .map(|(x, y)| {
            let v = seed
                .wrapping_add((x * 3001) as u16)
                .wrapping_add((y * 7919) as u16);
            [v, v.wrapping_mul(5), 65535 - v, 65535]
        })
        .collect();
    PrecisionImage::from_rgba16(width, height, pixels, IccProfile::srgb(), None).unwrap()
}

fn raster(name: &str, image: PrecisionImage, at: (i32, i32), opacity: f32) -> Layer {
    Layer::raster(name, image, at, opacity, true).unwrap()
}

fn document(size: (u32, u32), layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        size.0,
        size.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("a document")
}

fn styled(layer: Layer, json: &str, canvas: (u32, u32)) -> Layer {
    layer
        .with_layer_style(Some(&LayerStyle::from_json(json).unwrap()), canvas)
        .unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn invert(name: &str) -> Layer {
    let description =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    Layer::adjustment(name, &description, 1.0, true, &IccProfile::srgb()).unwrap()
}

/// The issue's 4×4 probe: an opaque Base under a half-alpha Top.
fn probe() -> PrecisionDocument {
    document(
        (4, 4),
        vec![
            raster("Base", image(4, 4, [20000, 0, 32768, 65535]), (0, 0), 1.0),
            raster("Top", image(4, 4, [60000, 40000, 8000, 32768]), (0, 0), 1.0),
        ],
    )
}

fn float_fixture() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return None;
    }
    let base = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).expect("opens");
    assert!(base.is_float(), "the fixture is a float document");
    let mut error = ptr::null_mut();
    let doubled = unsafe { dr_precision_doc_duplicate_layer(&base, 0, &mut error) };
    assert!(!doubled.is_null(), "{:?}", take_error(error));
    let doubled = *unsafe { Box::from_raw(doubled) };
    Some(
        doubled
            .set_layer_opacity(1, 0.5)
            .expect("a float opacity edit")
            .expect("the opacity changed"),
    )
}

// ------------------------------------------------------- the headlines --

#[test]
fn crop_merge_down_and_canvas_size_back_restore_the_corner_at_16_bits() {
    let stack = probe();
    let before = render16(&stack);
    let want = over(
        unit16([20000, 0, 32768, 65535]),
        unit16([60000, 40000, 8000, 32768]),
        1.0,
    );

    let cropped = must(&stack, crop(2, 2));
    let merged = must(&cropped, json!({"op": "merge_down", "layer": 1}));
    assert_eq!(merged.dimensions(), (2, 2), "the canvas is still cropped");
    assert_eq!(
        placed(&merged, 0),
        (json!([0, 0]), json!([4, 4])),
        "the merged layer keeps the members' whole 4x4 union"
    );
    let restored = must(&merged, canvas_size(4, 4));
    let after = render16(&restored);
    // Pixel (3, 3) lay outside the 2x2 canvas through the merge.
    let corner = after[3 * 4 + 3];
    for c in 0..4 {
        assert!(
            (f64::from(corner[c]) - want[c] * 65535.0).abs() <= 1.0,
            "pixel (3,3) channel {c} is {} against the oracle's {}",
            corner[c],
            want[c] * 65535.0
        );
    }
    assert_eq!(
        corner,
        before[3 * 4 + 3],
        "pixel (3,3) is the pre-edit render's"
    );
    within16(&after, &before, 0, "the whole restored canvas");
}

#[test]
fn a_member_moved_partly_off_canvas_merges_whole_at_16_bits() {
    let d = (2, 2);
    let stack = must(&probe(), moved(&[1], d.0, d.1));
    let merged = must(&stack, json!({"op": "merge_down", "layer": 1}));
    assert_eq!(
        placed(&merged, 0),
        (json!([0, 0]), json!([6, 6])),
        "Base at (0,0) and Top at (2,2), both 4x4"
    );
    let back = must(&merged, moved(&[0], -d.0, -d.1));
    // The oracle: the SAME unmerged stack with BOTH members moved by −d.
    let oracle = must(&stack, moved(&[0, 1], -d.0, -d.1));
    within16(
        &render16(&back),
        &render16(&oracle),
        0,
        "merged then moved back",
    );
}

#[test]
fn crop_merge_down_and_canvas_size_back_restore_the_picture_at_float() {
    let Some(stack) = float_fixture() else {
        return;
    };
    let (w, h) = stack.dimensions();
    let before = render32(&stack);
    let cropped = must(&stack, crop(w / 2, h / 2));
    let merged = must(&cropped, json!({"op": "merge_down", "layer": 1}));
    assert!(merged.is_float(), "still a float document");
    assert_eq!(
        placed(&merged, 0).1,
        json!([w, h]),
        "the merged layer keeps the members' whole union"
    );
    let restored = must(&merged, canvas_size(w, h));
    within32(&render32(&restored), &before, "the restored float canvas");
}

#[test]
fn a_member_moved_partly_off_canvas_merges_whole_at_float() {
    let Some(base) = float_fixture() else {
        return;
    };
    let d = (3, 2);
    let stack = must(&base, moved(&[1], d.0, d.1));
    let merged = must(&stack, json!({"op": "merge_down", "layer": 1}));
    let (w, h) = base.dimensions();
    assert_eq!(placed(&merged, 0).1, json!([w + 3, h + 2]));
    let back = must(&merged, moved(&[0], -d.0, -d.1));
    let oracle = must(&stack, moved(&[0, 1], -d.0, -d.1));
    within32(
        &render32(&back),
        &render32(&oracle),
        "float merged then moved back",
    );
}

// ------------------------------------------------------- the extent shape --

#[test]
fn the_merged_extent_is_the_union_of_the_visible_members() {
    let opaque = [30000, 20000, 10000, 65535];
    // A NEGATIVE offset: the origin moves up and left.
    let negative = document(
        (4, 4),
        vec![
            raster("A", image(2, 2, opaque), (0, 0), 1.0),
            raster("B", image(2, 2, opaque), (-3, -2), 1.0),
        ],
    );
    let merged = must(&negative, json!({"op": "merge_down", "layer": 1}));
    assert_eq!(placed(&merged, 0), (json!([-3, -2]), json!([5, 4])));

    // A union SMALLER than the canvas: no canvas-sized leaf.
    let small = document(
        (8, 8),
        vec![
            raster("A", image(2, 2, opaque), (1, 1), 1.0),
            raster("B", image(2, 2, opaque), (2, 3), 1.0),
        ],
    );
    let merged = must(&small, json!({"op": "merge_layers", "layers": [0, 1]}));
    assert_eq!(placed(&merged, 0), (json!([1, 1]), json!([3, 4])));

    // A CLIPPED upper member counts in full, as at 8 bits.
    let clipped = document(
        (4, 4),
        vec![
            raster("Base", image(2, 2, opaque), (1, 1), 1.0),
            raster("Clip", image(8, 8, [65535, 0, 0, 32768]), (-2, -2), 1.0).with_clipped(true),
        ],
    );
    let before = render16(&clipped);
    let merged = must(&clipped, json!({"op": "merge_down", "layer": 1}));
    assert_eq!(placed(&merged, 0), (json!([-2, -2]), json!([8, 8])));
    within16(
        &render16(&merged),
        &before,
        1,
        "the clip unit bakes as it projects",
    );

    // ADJUSTMENTS alone count as the canvas, their byte twin's pixels.
    let adjustments = document((5, 3), vec![invert("One"), invert("Two")]);
    let merged = must(
        &adjustments,
        json!({"op": "merge_layers", "layers": [0, 1]}),
    );
    assert_eq!(placed(&merged, 0), (json!([0, 0]), json!([5, 3])));

    // Two 2x2 members at (0,0) and (1,1) on a 2x2 canvas: a 3x3 union, and
    // the three quarters of the upper member off the canvas survive.
    let pair = document(
        (2, 2),
        vec![
            raster("Base", image(2, 2, [0, 0, 0, 65535]), (0, 0), 1.0),
            raster(
                "Offset",
                image(2, 2, [65535, 65535, 65535, 65535]),
                (1, 1),
                1.0,
            ),
        ],
    );
    let merged = must(&pair, json!({"op": "merge_down", "layer": 1}));
    assert_eq!(placed(&merged, 0), (json!([0, 0]), json!([3, 3])));
    let got = samples(&merged, 0);
    assert_eq!(got[0], [0, 0, 0, 65535], "top-left keeps the base");
    assert_eq!(got[4], [65535, 65535, 65535, 65535], "(1,1) is covered");
    assert_eq!(
        got[8],
        [65535, 65535, 65535, 65535],
        "(2,2), off the canvas, survives"
    );
    assert_eq!(got[2], [0, 0, 0, 0], "(2,0) belongs to no member");
}

// ------------------------------------------- parity with the projection --

/// The canvas render before and after `request` agree within one code, and
/// record `index` of the result sits at a non-(0,0) origin.
fn parity(label: &str, stack: &PrecisionDocument, request: Value, index: usize) {
    let before = render16(stack);
    let merged = must(stack, request);
    let (offset, _) = placed(&merged, index);
    assert_ne!(offset, json!([0, 0]), "{label}: the frame's origin moved");
    within16(&render16(&merged), &before, 1, label);
}

const W: (u32, u32) = (8, 8);

fn base() -> Layer {
    raster("Base", pattern(8, 8, 101), (0, 0), 1.0)
}

#[test]
fn an_offset_mask_bakes_at_its_document_place() {
    let mut properties = MaskProperties::at((1, -1));
    properties.density = 1.0;
    let samples: Vec<u16> = (0..16).map(|i| (i * 4369) as u16).collect();
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(4, 4, samples, None).unwrap(),
        properties,
        true,
    )
    .unwrap();
    let stack = document(
        W,
        vec![
            base(),
            raster("A", pattern(5, 5, 7), (-2, -1), 1.0).with_mask(Some(mask)),
            raster("B", image(3, 3, [9000, 50000, 20000, 40000]), (4, 4), 1.0),
        ],
    );
    parity(
        "an offset mask",
        &stack,
        json!({"op": "merge_layers", "layers": [1, 2]}),
        1,
    );
}

#[test]
fn dissolve_dithers_at_document_coordinates() {
    let stack = document(
        W,
        vec![
            base(),
            raster("A", pattern(4, 4, 9), (-1, -2), 1.0),
            raster("Dissolve", pattern(9, 9, 3001), (-1, -1), 0.5)
                .with_blend_mode(BlendMode::Dissolve)
                .unwrap(),
        ],
    );
    parity(
        "dissolve",
        &stack,
        json!({"op": "merge_down", "layer": 2}),
        1,
    );
}

#[test]
fn a_canvas_aligned_gradient_overlay_bakes_where_it_projects() {
    let gradient = r##"{"effects":[{"type":"gradient_overlay","opacity":0.8,
        "gradient":{"style":"linear","angle":30,"align_with_layer":false,
        "stops":[{"position":0,"color":"#ff0000","opacity":1},
                 {"position":1,"color":"#0000ff","opacity":1}]}}]}"##;
    let stack = document(
        W,
        vec![
            base(),
            styled(
                raster("Gradient", pattern(6, 6, 11), (-2, -3), 1.0),
                gradient,
                W,
            ),
            raster("B", image(2, 2, [1000, 2000, 3000, 65535]), (5, 5), 1.0),
        ],
    );
    parity(
        "a gradient overlay",
        &stack,
        json!({"op": "merge_layers", "layers": [1, 2]}),
        1,
    );
}

#[test]
fn a_drop_shadow_reaching_off_canvas_bakes_where_it_projects() {
    let shadow = r#"{"effects":[{"type":"drop_shadow","distance":3,"size":2,"angle":45,
        "use_global_light":false,"opacity":0.9}]}"#;
    let stack = document(
        W,
        vec![
            base(),
            styled(
                raster("Shadowed", pattern(4, 4, 13), (-2, -1), 1.0),
                shadow,
                W,
            ),
            raster("B", image(2, 2, [1000, 2000, 3000, 65535]), (6, 6), 1.0),
        ],
    );
    parity(
        "a drop shadow",
        &stack,
        json!({"op": "merge_layers", "layers": [1, 2]}),
        1,
    );
}

#[test]
fn a_clip_run_bakes_where_it_projects() {
    let stack = document(
        W,
        vec![
            base(),
            raster(
                "Clip base",
                image(4, 4, [30000, 30000, 30000, 50000]),
                (-1, -1),
                1.0,
            ),
            raster("Member", pattern(10, 10, 17), (-2, -2), 0.7).with_clipped(true),
        ],
    );
    parity(
        "a clip run",
        &stack,
        json!({"op": "merge_down", "layer": 2}),
        1,
    );
}

#[test]
fn a_pass_through_member_bakes_where_it_projects() {
    let child = raster("Child", pattern(5, 5, 19), (-3, -1), 1.0);
    let group = Layer::pass_through("Pass", vec![child], 0.6, true).unwrap();
    let stack = document(
        W,
        vec![
            group,
            raster(
                "Top",
                image(3, 3, [50000, 10000, 10000, 30000]),
                (4, 2),
                1.0,
            ),
        ],
    );
    // Records: Child 0, Pass 1, Top 2. Nothing lies below the group, so the
    // members' private accumulator IS the backdrop it passes through to.
    parity(
        "pass through",
        &stack,
        json!({"op": "merge_layers", "layers": [1, 2]}),
        0,
    );
}

#[test]
fn a_knockout_punches_to_the_real_backdrop_and_to_transparency_off_canvas() {
    let knockout = r#"{"knockout":"shallow","fill_opacity":0}"#;
    let stack = document(
        W,
        vec![
            raster("Background", pattern(8, 8, 23), (0, 0), 1.0),
            raster("A", image(5, 5, [60000, 5000, 5000, 65535]), (-2, -2), 1.0),
            styled(
                raster("Hole", image(4, 4, [0, 0, 0, 65535]), (-2, -2), 1.0),
                knockout,
                W,
            ),
        ],
    );
    parity(
        "knockout",
        &stack,
        json!({"op": "merge_layers", "layers": [1, 2]}),
        1,
    );
    let merged = must(&stack, json!({"op": "merge_layers", "layers": [1, 2]}));
    assert_eq!(placed(&merged, 1), (json!([-2, -2]), json!([5, 5])));
    let got = samples(&merged, 1);
    assert_eq!(got[0][3], 0, "off the canvas the hole shows transparency");
    let background = samples(&stack, 0);
    assert_eq!(
        got[2 * 5 + 2],
        background[0],
        "on the canvas the hole holds the Background's pixel"
    );
}

// ------------------------------------------------------------ byte parity --

fn group_many(doc: *mut DrDocument, idx: &[usize]) -> *mut DrDocument {
    let name = CString::new("Group").unwrap();
    let cap = unsafe { dr_doc_layer_count(doc) } + 1;
    let (mut group, mut cleared_len, mut reordered_len) = (usize::MAX, 0usize, 0usize);
    let mut cleared = vec![0usize; cap];
    let mut reordered = vec![0usize; cap];
    let out = unsafe {
        dr_doc_group_layers(
            doc,
            idx.as_ptr(),
            idx.len(),
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            cap,
        )
    };
    assert!(!out.is_null(), "group_layers({idx:?}) refused");
    unsafe { dr_doc_free(doc) };
    out
}

/// Records: Base 0 (4x4), A 1 (3x3 at (-2,-1), half alpha), B 2 (3x3 at
/// (2,3), inside Group 3) — so the group member holds off-canvas content.
fn byte_stack() -> *mut DrDocument {
    let mut doc = DrDocument::from_pixels(solid(4, 4, [40, 80, 120, 255]))
        .adding_image_layer(0, solid(3, 3, [200, 20, 60, 128]), "A")
        .expect("A")
        .adding_image_layer(1, solid(3, 3, [10, 220, 90, 255]), "B")
        .expect("B");
    doc.layers[1].offset = (-2, -1);
    doc.layers[2].offset = (2, 3);
    group_many(Box::into_raw(Box::new(doc)), &[2])
}

fn promoted(doc: *const DrDocument) -> PrecisionDocument {
    let mut error = ptr::null_mut();
    let out = unsafe { dr_doc_promote_structure(doc, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!out.is_null(), "{:?}", take_error(error));
    *unsafe { Box::from_raw(out) }
}

fn byte_against_typed(label: &str, byte: *mut DrDocument, typed: &PrecisionDocument, index: usize) {
    let (bw, bh) = layer_dims(byte, index);
    let (bx, by) = layer_offset(byte, index);
    assert_eq!(
        placed(typed, index),
        (json!([bx, by]), json!([bw, bh])),
        "{label}: the typed merge lands where the byte merge does"
    );
    let bytes = layer_pixels(byte, index);
    let want: Vec<[u16; 4]> = bytes
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257))
        .collect();
    within16(&samples(typed, index), &want, 257, label);
    unsafe { dr_doc_free(byte) };
}

#[test]
fn the_typed_merges_match_the_byte_merges_of_the_promoted_stack() {
    let source = byte_stack();
    let typed = promoted(source);
    let members = [1usize, 3];
    let byte = unsafe { dr_doc_merge_layers(source, members.as_ptr(), members.len()) };
    assert!(!byte.is_null(), "the byte Merge Layers");
    let merged = must(&typed, json!({"op": "merge_layers", "layers": [1, 3]}));
    byte_against_typed("merge layers", byte, &merged, 1);

    let byte = unsafe { dr_doc_merging_down(source, 3) };
    assert!(!byte.is_null(), "the byte Merge Down");
    let merged = must(&typed, json!({"op": "merge_down", "layer": 3}));
    byte_against_typed("merge down", byte, &merged, 1);
    unsafe { dr_doc_free(source) };
}

// ------------------------------------------------- the masked adjustment --

/// The ONE stated difference from 8 bits: a typed masked adjustment gates
/// the whole frame, reading its mask's OWN `outside_coverage` beyond the
/// plane, where the byte twin skips every pixel outside its mask plane.
#[test]
fn a_masked_adjustment_gates_off_canvas_pixels_by_its_outside_coverage() {
    let color = [10000, 20000, 30000, 65535];
    let inverted = [55535, 45535, 35535, 65535];
    for (outside, off_canvas) in [(0u8, color), (255u8, inverted)] {
        let mut properties = MaskProperties::at((0, 0));
        properties.outside_coverage = outside;
        let mask = PrecisionMask::new(
            PrecisionPlane::from_u16(4, 4, vec![65535; 16], None).unwrap(),
            properties,
            true,
        )
        .unwrap();
        let stack = document(
            (4, 4),
            vec![
                raster("Base", image(4, 4, color), (-2, -2), 1.0),
                invert("Invert").with_mask(Some(mask)),
            ],
        );
        let merged = must(&stack, json!({"op": "merge_layers", "layers": [0, 1]}));
        assert_eq!(placed(&merged, 0), (json!([-2, -2]), json!([6, 6])));
        let got = samples(&merged, 0);
        within16(
            &[got[0]],
            &[off_canvas],
            1,
            "document (-2,-2), off the canvas",
        );
        within16(
            &[got[2 * 6 + 2]],
            &[inverted],
            1,
            "document (0,0), inside the mask",
        );
    }
}

// ------------------------------------------------------------ refusals --

#[test]
fn an_empty_union_an_oversized_union_and_an_out_of_range_union_are_refused() {
    // The only visible member is an EMPTY group.
    let empty = document(
        (2, 2),
        vec![
            Layer::isolated_normal("Empty", vec![], 1.0, true).unwrap(),
            Layer::raster("Hidden", image(2, 2, [1, 2, 3, 65535]), (0, 0), 1.0, false).unwrap(),
        ],
    );
    let request = json!({"op": "merge_layers", "layers": [0, 1]});
    let nothing = "high-depth merge has nothing visible to merge";
    assert_eq!(refusal(&empty, request.clone()), nothing);
    assert_eq!(plan_refusal(&empty, request), nothing);

    // 1x1 at (0,0) and 1x1 at (10000,10001): a 10001x10002 union.
    let far = document(
        (2, 2),
        vec![
            raster("Near", image(1, 1, [1, 2, 3, 65535]), (0, 0), 1.0),
            raster("Far", image(1, 1, [4, 5, 6, 65535]), (10000, 10001), 1.0),
        ],
    );
    let request = json!({"op": "merge_down", "layer": 1});
    let oversize = "high-depth merge covers 10001×10002 pixels, over the pixel limit; \
                    crop, trim or scale the layers first";
    assert_eq!(refusal(&far, request.clone()), oversize);
    assert_eq!(plan_refusal(&far, request), oversize);

    // A styled member at x = i32::MIN: its pad reaches past the range.
    let edge = document(
        (2, 2),
        vec![
            raster("Plain", image(1, 1, [1, 2, 3, 65535]), (i32::MIN, 0), 1.0),
            styled(
                raster("Stroked", image(1, 1, [4, 5, 6, 65535]), (i32::MIN, 0), 1.0),
                r#"{"effects":[{"type":"stroke","size":3}]}"#,
                (2, 2),
            ),
        ],
    );
    let request = json!({"op": "merge_layers", "layers": [0, 1]});
    let range = "high-depth merge extends outside the supported coordinate range";
    assert_eq!(refusal(&edge, request.clone()), range);
    assert_eq!(plan_refusal(&edge, request), range);
}

#[test]
fn merge_down_of_an_empty_visible_group_removes_it() {
    let stack = document(
        (2, 2),
        vec![
            raster("Base", image(2, 2, [1, 2, 3, 65535]), (0, 0), 1.0),
            Layer::isolated_normal("Empty", vec![], 1.0, true).unwrap(),
        ],
    );
    let request = json!({"op": "merge_down", "layer": 1});
    // The plan charges no render for the removal path, and does not refuse.
    plan_scratch(&stack, request.clone());
    let merged = must(&stack, request);
    assert_eq!(merged.layer_count(), 1, "the empty group is gone");
    assert_eq!(rows(&merged)[0]["name"], "Base");
    assert_eq!(
        samples(&merged, 0),
        samples(&stack, 0),
        "the base is intact"
    );
}

// ----------------------------------------------------------------- plan --

#[test]
fn the_plan_charges_the_union_rather_than_the_canvas() {
    let small = document(
        (64, 64),
        vec![
            raster("A", image(2, 2, [1, 2, 3, 65535]), (0, 0), 1.0),
            raster("B", image(2, 2, [4, 5, 6, 32768]), (0, 0), 1.0),
        ],
    );
    let large = document(
        (64, 64),
        vec![
            raster("A", image(64, 64, [1, 2, 3, 65535]), (0, 0), 1.0),
            raster("B", image(64, 64, [4, 5, 6, 32768]), (0, 0), 1.0),
        ],
    );
    let request = json!({"op": "merge_layers", "layers": [0, 1]});
    let union = plan_scratch(&small, request.clone());
    let canvas = plan_scratch(&large, request);
    assert!(
        union < canvas,
        "a 2x2 union charges {union} bytes, two 64x64 members {canvas}"
    );
}
