//! Geometry on a CMYK document (#547 phase 4b): Image Size, transforms and
//! perspective of the CMYKA16 ink against `common/cmyk_geometry_fixture.rs`'s
//! f64 premultiplied reference resampler (≤ 1 code a plate, alpha included,
//! at every pixel and kernel), the exact permutations, every geometry op
//! plane for plane against its two RGB16 twins, Trim over the composite
//! against a scan written here, the content box (alpha × mask) that Reveal
//! All, align and distribute read, the identity, the plans and the
//! capability rows.
#![allow(clippy::needless_range_loop)]

use std::ptr;

use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::ffi_precision_inspect::dr_precision_doc_layer_bounds;
use doctored_core::ffi_trim::dr_precision_doc_cmyk_trim_rect;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::json;

use crate::cmyk_geometry_fixture::{
    affine_inverse, assert_twin_edit, assert_twins, assert_within, each_edit, homography, invert3,
    record, resize_oracle, transform_oracle, turn, twins, Cmyk, Kernel, Twins,
};
use crate::cmyk_structure_fixture::{
    cmyk_layers, composite, free, json, plan, ramp, try_edit, varied, with_mask, RgbLayer, LIMIT,
};
use crate::common::take_err_string;

const W: u32 = 37;
const H: u32 = 23;
const KERNELS: [Kernel; 3] = [Kernel::Bilinear, Kernel::Bicubic, Kernel::Lanczos3];

/// A whole-canvas permutation: its request, output size and source pixel.
type Permutation<'a> = (&'a str, (u32, u32), &'a dyn Fn(u32, u32) -> Cmyk);

/// The 37 × 23 fixture: one full-canvas record with every alpha class.
fn fixture() -> *mut PrecisionDocument {
    cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Ink",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: varied(W, H, 11),
        }],
    )
}

/// The fixture with a canvas mask, and its twins.
fn masked_twins() -> Twins {
    twins(with_mask(fixture(), 0, &ramp(W, H, 5)))
}

fn edited(doc: *const PrecisionDocument, request: &str) -> *mut PrecisionDocument {
    let out = try_edit(doc, request, &[]).unwrap_or_else(|e| panic!("{request}: {e}"));
    assert!(!out.is_null(), "{request} changed nothing");
    out
}

#[test]
fn resize_up_and_down_matches_the_f64_oracle_at_every_kernel() {
    let doc = fixture();
    let (source, _, size) = record(doc, 0);
    for kernel in KERNELS {
        for target in [(55, 34), (19, 11), (40, 17)] {
            let request = json!({"op":"resize","width":target.0,"height":target.1,
                "filter":kernel.name()})
            .to_string();
            let out = edited(doc, &request);
            let (got, offset, got_size) = record(out, 0);
            assert_eq!((offset, got_size), ((0, 0), target), "{request}");
            let want = resize_oracle(&source, size, target, kernel);
            assert_within(&got, &want, 1, &request);
            free(out);
        }
    }
    free(doc);
}

#[test]
fn affine_and_perspective_match_the_f64_oracle_at_every_kernel() {
    let doc = fixture();
    let (source, origin, size) = record(doc, 0);
    let matrix = turn(17.0, 1.15, (18.5, 11.5));
    let corners = [(0.0, 0.0), (37.0, 0.0), (37.0, 23.0), (0.0, 23.0)];
    let quad = [(3.0, 2.0), (35.0, 5.0), (31.0, 21.0), (6.0, 18.0)];
    let flat: Vec<f64> = quad.iter().flat_map(|p| [p.0, p.1]).collect();
    for kernel in KERNELS {
        let cases = [
            (
                json!({"op":"affine","layer":0,"target":"pixels","matrix":matrix,
                    "filter":kernel.name()}),
                affine_inverse(matrix),
            ),
            (
                json!({"op":"perspective","layer":0,"target":"pixels","quad":flat,
                    "filter":kernel.name()}),
                invert3(homography(corners, quad)),
            ),
        ];
        for (request, inverse) in cases {
            let request = request.to_string();
            let out = edited(doc, &request);
            let (got, offset, got_size) = record(out, 0);
            let want = transform_oracle(&source, size, origin, inverse, (offset, got_size), kernel);
            assert_within(&got, &want, 1, &request);
            free(out);
        }
    }
    free(doc);
}

#[test]
fn nearest_quarter_turns_and_flips_are_exact_permutations() {
    let doc = fixture();
    let (source, _, _) = record(doc, 0);
    let at = |x: u32, y: u32| source[(y * W + x) as usize];
    let cases: [Permutation<'_>; 5] = [
        (r#"{"op":"rotate90"}"#, (H, W), &|x, y| at(y, H - 1 - x)),
        (r#"{"op":"rotate180"}"#, (W, H), &|x, y| {
            at(W - 1 - x, H - 1 - y)
        }),
        (r#"{"op":"rotate270"}"#, (H, W), &|x, y| at(W - 1 - y, x)),
        (r#"{"op":"flip_horizontal"}"#, (W, H), &|x, y| {
            at(W - 1 - x, y)
        }),
        (r#"{"op":"flip_vertical"}"#, (W, H), &|x, y| {
            at(x, H - 1 - y)
        }),
    ];
    for (request, size, from) in cases {
        let out = edited(doc, request);
        let (got, _, got_size) = record(out, 0);
        assert_eq!(got_size, size, "{request}");
        for y in 0..size.1 {
            for x in 0..size.0 {
                assert_eq!(
                    got[(y * size.0 + x) as usize],
                    from(x, y),
                    "{request} ({x}, {y})"
                );
            }
        }
        free(out);
    }
    // Nearest resize picks one source pixel per output pixel, bit for bit.
    let out = edited(
        doc,
        r#"{"op":"resize","width":50,"height":9,"filter":"nearest"}"#,
    );
    let (got, _, _) = record(out, 0);
    for y in 0..9u64 {
        for x in 0..50u64 {
            let sy = ((2 * y + 1) * u64::from(H) / 18) as u32;
            let sx = ((2 * x + 1) * u64::from(W) / 100) as u32;
            assert_eq!(got[(y * 50 + x) as usize], at(sx, sy), "nearest ({x}, {y})");
        }
    }
    free(out);
    free(doc);
}

#[test]
fn every_geometry_op_is_its_rgb16_twins_plane_for_plane() {
    let base = masked_twins();
    let turn17 = turn(17.0, 1.0, (18.5, 11.5));
    let quad = [3.0, 2.0, 35.0, 5.0, 31.0, 21.0, 6.0, 18.0];
    let requests = [
        json!({"op":"resize","width":51,"height":30,"filter":"bicubic"}),
        json!({"op":"resize","width":20,"height":13,"filter":"lanczos3"}),
        json!({"op":"crop","x":4,"y":3,"width":25,"height":15}),
        json!({"op":"canvas_size","width":45,"height":30,"x":5,"y":-3}),
        json!({"op":"straighten","matrix":turn17,"filter":"bilinear","x":3,"y":2,"width":30,"height":18}),
        json!({"op":"straighten","matrix":turn17,"filter":"bicubic","x":3,"y":2,"width":30,
            "height":18,"delete_cropped":true}),
        json!({"op":"rotate_canvas","degrees":23.0,"filter":"bicubic"}),
        json!({"op":"rotate90"}),
        json!({"op":"flip_vertical"}),
        json!({"op":"affine","layer":0,"target":"pixels","matrix":turn17,"filter":"lanczos3"}),
        json!({"op":"affine","layer":0,"target":"mask","matrix":turn17,"filter":"bilinear"}),
        json!({"op":"perspective","layer":0,"target":"pixels","quad":quad,"filter":"bicubic"}),
        json!({"op":"affine_set","layers":[0],"matrix":turn(9.0, 0.8, (10.0, 4.0)),
            "filter":"bilinear"}),
        json!({"op":"perspective_set","layers":[0],"source_rect":[0.0,0.0,37.0,23.0],
            "quad":quad,"filter":"bilinear"}),
        // The Move tool: an integer affine set, a Keep plane.
        json!({"op":"affine_set","layers":[0],"matrix":[1.0,0.0,0.0,1.0,-6.0,4.0],
            "filter":"nearest"}),
    ];
    for request in requests {
        let out = assert_twin_edit(&base, &request.to_string(), &[]);
        out.free();
    }
    base.free();
}

#[test]
fn an_identity_transform_answers_no_change() {
    let base = masked_twins();
    for request in [
        json!({"op":"affine","layer":0,"target":"pixels","matrix":[1.0,0.0,0.0,1.0,0.0,0.0],
            "filter":"bicubic"}),
        json!({"op":"resize","width":W,"height":H,"filter":"bicubic"}),
        json!({"op":"crop","x":0,"y":0,"width":W,"height":H}),
        json!({"op":"canvas_size","width":W,"height":H,"x":0,"y":0}),
    ] {
        assert!(
            each_edit(&base, &request.to_string(), &[]).is_none(),
            "{request} changes nothing"
        );
    }
    base.free();
}

/// The composite's straight samples, row-major.
fn composite_of(doc: *const PrecisionDocument) -> Vec<Cmyk> {
    let (w, h) = unsafe { &*doc }.dimensions();
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| composite(doc, x as i32, y as i32))
        .collect()
}

/// This file's own trim scan: the box of the pixels the basis keeps, each
/// unchecked edge left where the canvas has it.
fn trim_scan(pixels: &[Cmyk], basis: u32, edges: u32) -> Option<[u32; 4]> {
    let corner = match basis {
        1 => Some(pixels[0]),
        2 => Some(pixels[pixels.len() - 1]),
        _ => None,
    };
    let keeps = |p: &Cmyk| corner.map_or(p[4] != 0, |c| *p != c);
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, p) in pixels.iter().enumerate() {
        if keeps(p) {
            let (x, y) = (i as u32 % W, i as u32 / W);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    if x0 == u32::MAX {
        return None;
    }
    let (l, t) = (
        if edges & 4 != 0 { x0 } else { 0 },
        if edges & 1 != 0 { y0 } else { 0 },
    );
    let (r, b) = (
        if edges & 8 != 0 { x1 } else { W - 1 },
        if edges & 2 != 0 { y1 } else { H - 1 },
    );
    Some([l, t, r - l + 1, b - t + 1])
}

fn trim(doc: *const PrecisionDocument, basis: i32, edges: u32) -> Option<[u32; 4]> {
    let mut rect = [0u32; 4];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_cmyk_trim_rect(doc, basis, edges, LIMIT, 0, rect.as_mut_ptr(), &mut err)
    };
    assert!(err.is_null(), "trim: {}", take_err_string(err));
    ok.then_some(rect)
}

#[test]
fn trim_keeps_the_composites_box_at_every_basis_and_edge_set() {
    // A 4-px border: transparent on one record, a solid opaque colour under
    // it on another (so the bases differ), varied ink inside.
    let border = |x: u32, y: u32| x < 4 || y < 4 || x >= W - 5 || y >= H - 3;
    let inner: Vec<[u16; 4]> = varied(W, H, 3)
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            if border(i as u32 % W, i as u32 / W) {
                [0; 4]
            } else {
                p
            }
        })
        .collect();
    let solid: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            if border(i % W, i / W) {
                [9000, 30000, 52000, 65535]
            } else {
                [0; 4]
            }
        })
        .collect();
    for layers in [
        vec![("Inner", inner.clone())],
        vec![("Solid", solid), ("Inner", inner)],
    ] {
        let doc = cmyk_layers(
            W,
            H,
            layers
                .into_iter()
                .map(|(name, pixels)| RgbLayer {
                    name,
                    w: W,
                    h: H,
                    offset: (0, 0),
                    pixels,
                })
                .collect(),
        );
        let pixels = composite_of(doc);
        for basis in 0..3 {
            for edges in [15, 1, 2, 4, 8, 5, 10] {
                assert_eq!(
                    trim(doc, basis, edges),
                    trim_scan(&pixels, basis as u32, edges),
                    "basis {basis} edges {edges}"
                );
            }
        }
        free(doc);
    }
    // An unknown basis keeps nothing and refuses nothing.
    let doc = fixture();
    assert_eq!(trim(doc, 7, 15), None);
    free(doc);
}

fn bounds(doc: *const PrecisionDocument, index: usize) -> Option<[i32; 4]> {
    let mut out = [0i32; 4];
    unsafe { dr_precision_doc_layer_bounds(doc, index, out.as_mut_ptr()) }.then_some(out)
}

/// This file's own box: alpha > 0 and mask > 0, in canvas coordinates.
fn box_scan(doc: *const PrecisionDocument, index: usize, mask: Option<&[u16]>) -> [i32; 4] {
    let (samples, offset, (w, _)) = record(doc, index);
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (i, p) in samples.iter().enumerate() {
        let (x, y) = (
            offset.0 + (i as u32 % w) as i32,
            offset.1 + (i as u32 / w) as i32,
        );
        let masked = mask.is_some_and(|m| {
            x < 0
                || y < 0
                || x >= W as i32
                || y >= H as i32
                || m[(y as u32 * W + x as u32) as usize] == 0
        });
        if p[4] > 0 && !masked {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    [x0, y0, x1 - x0 + 1, y1 - y0 + 1]
}

#[test]
fn the_content_box_is_alpha_times_mask_and_align_and_distribute_read_it() {
    let shaped = |x0: u32, y0: u32, x1: u32, y1: u32, seed: u32| -> Vec<[u16; 4]> {
        varied(W, H, seed)
            .into_iter()
            .enumerate()
            .map(|(i, mut p)| {
                let (x, y) = (i as u32 % W, i as u32 / W);
                if x < x0 || y < y0 || x >= x1 || y >= y1 {
                    p[3] = 0;
                } else if p[3] == 0 {
                    p[3] = 30000;
                }
                p
            })
            .collect()
    };
    let layer = |name, pixels| RgbLayer {
        name,
        w: W,
        h: H,
        offset: (0, 0),
        pixels,
    };
    let doc = cmyk_layers(
        W,
        H,
        vec![
            layer("A", shaped(5, 4, 12, 19, 1)),
            layer("B", shaped(15, 2, 22, 10, 2)),
            layer("C", shaped(27, 8, 33, 21, 3)),
        ],
    );
    for i in 0..3 {
        assert_eq!(bounds(doc, i), Some(box_scan(doc, i, None)), "record {i}");
    }
    // A mask hiding the left half of B shrinks its box.
    let mask: Vec<u16> = (0..W * H)
        .map(|i| if i % W < 18 { 0 } else { 65535 })
        .collect();
    let doc = with_mask(doc, 1, &mask);
    assert_eq!(bounds(doc, 1), Some(box_scan(doc, 1, Some(&mask))));
    // Align to the canvas's left edge moves each box's LEFT to 0.
    let aligned = edited(
        doc,
        r#"{"op":"align_set","layers":[0,2],"edge":"left","to_canvas":true}"#,
    );
    for i in [0, 2] {
        assert_eq!(bounds(aligned, i).unwrap()[0], 0, "record {i}");
    }
    free(aligned);
    // Align and distribute equal their RGB16 twins (whose box is the same
    // alpha × mask rule).
    let base = twins(doc);
    for request in [
        r#"{"op":"align_set","layers":[0,1,2],"edge":"top","to_canvas":false}"#,
        r#"{"op":"align_set","layers":[0,1,2],"edge":"right","to_canvas":false}"#,
        r#"{"op":"distribute_set","layers":[0,1,2],"vertical":true}"#,
    ] {
        assert_twin_edit(&base, request, &[]).free();
    }
    // A horizontal distribute these boxes already satisfy is no change on
    // all three alike (`each_edit` asserts the agreement).
    if let Some(out) = each_edit(
        &base,
        r#"{"op":"distribute_set","layers":[0,1,2],"vertical":false}"#,
        &[],
    ) {
        assert_twins(&out, "distribute horizontally");
        out.free();
    }
    // Reveal All's input: a record moved partly off the canvas boxes its
    // content where it now stands, past the canvas edge.
    let moved = edited(
        base.cmyk,
        r#"{"op":"affine_set","layers":[2],"matrix":[1.0,0.0,0.0,1.0,12.0,-9.0],"filter":"nearest"}"#,
    );
    let before = bounds(base.cmyk, 2).unwrap();
    assert_eq!(
        bounds(moved, 2),
        Some([before[0] + 12, before[1] - 9, before[2], before[3]])
    );
    assert_eq!(bounds(moved, 2), Some(box_scan(moved, 2, None)));
    free(moved);
    base.free();
}

#[test]
fn the_geometry_plans_admit_and_the_descriptor_reports_them() {
    // 1200 × 900, so the descriptor's fixed warp (a node moved (60, 40) on a
    // 4 × 4 lattice) is within the field's non-folding bound.
    let (w, h) = (1200, 900);
    let doc = with_mask(
        cmyk_layers(
            w,
            h,
            vec![RgbLayer {
                name: "Ink",
                w,
                h,
                offset: (0, 0),
                pixels: varied(w, h, 4),
            }],
        ),
        0,
        &ramp(w, h, 2),
    );
    for request in [
        r#"{"op":"resize","width":56,"height":35,"filter":"bicubic"}"#,
        r#"{"op":"rotate90"}"#,
        r#"{"op":"perspective_crop","quad":[3,2,34,4,33,21,2,20],"width":30,"height":18,"filter":"bilinear"}"#,
        r#"{"op":"align_set","layers":[0],"edge":"left","to_canvas":true}"#,
    ] {
        let value = plan(doc, request).unwrap_or_else(|e| panic!("{request}: {e}"));
        assert!(
            value["scratch_bytes"].as_u64().unwrap() > 0,
            "{request}: {value}"
        );
    }
    let mut err = ptr::null_mut();
    let descriptor = json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err);
    for key in [
        "cmyk_resize_scratch_bytes",
        "cmyk_affine_scratch_bytes",
        "cmyk_perspective_crop_scratch_bytes",
        "cmyk_warp_scratch_bytes",
        "cmyk_liquify_scratch_bytes",
        "cmyk_transform_selected_scratch_bytes",
    ] {
        assert!(
            descriptor[key].as_u64().is_some_and(|v| v > 0),
            "{key}: {}",
            descriptor[key]
        );
    }
    let operations = descriptor["supported_operations"].to_string();
    for family in [
        "geometry_resize",
        "geometry_warp",
        "mask_transform",
        "selected_pixels_move",
    ] {
        assert!(operations.contains(family), "{family} in {operations}");
    }
    free(doc);
}

#[test]
fn the_twins_hold_a_masked_document_unchanged() {
    // The twin builder itself: an untouched document is its twins.
    let base = masked_twins();
    assert_twins(&base, "untouched");
    base.free();
}
